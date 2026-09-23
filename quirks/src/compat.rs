// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The compatibility layer: a [`Bmc`] over another that applies the
//! classified platform's document repairs to everything it reads.
//!
//! [`CompatBmc`] fetches each document raw, applies the repairs enabled for
//! the platform to every object carrying an `@odata.type` — members a
//! device expanded inline included, at any depth — and only then
//! deserializes into the type the caller asked for. Everything it does not
//! read it delegates unchanged. It is classified once, from the service
//! root, with one request; an object without `@odata.type` gets no repair,
//! and a platform that needs no repair is deserialized without a copy. The
//! raw document carries the device's `@odata.etag`, so a caching transport
//! revalidates reads through the layer exactly as it does typed ones.

use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use futures_util::TryStreamExt as _;
use nv_redfish_core::query::ExpandQuery;
use nv_redfish_core::Action;
use nv_redfish_core::Bmc;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
use nv_redfish_core::FilterQuery;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::HttpPushUriUpdateRequest;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::MultipartUpdateRequest;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::SessionCreateResponse;
use nv_redfish_core::StreamEvent;
use nv_redfish_core::UploadReader;
use serde::Deserialize;
use serde::Serialize;

use crate::rules;
use crate::BmcQuirks;
use crate::Raw;
use crate::RootEvidence;

/// What a read through [`CompatBmc`] can fail with.
#[derive(Debug)]
pub enum CompatError<E> {
    /// The transport underneath failed.
    Transport(E),
    /// The document, repaired, still did not deserialize into the type
    /// asked for.
    Decode(serde_json::Error),
}

impl<E: fmt::Display> fmt::Display for CompatError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "BMC error: {error}"),
            Self::Decode(error) => write!(f, "repaired document did not deserialize: {error}"),
        }
    }
}

impl<E: StdError + 'static> StdError for CompatError<E> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Decode(error) => Some(error),
        }
    }
}

/// A [`Bmc`] that repairs what it reads for the platform it was classified
/// against, and delegates everything else to the transport underneath.
pub struct CompatBmc<B: Bmc> {
    inner: Arc<B>,
    quirks: Arc<BmcQuirks>,
    /// Whether the platform enables any repair; when it does not, documents
    /// deserialize in place.
    repairs: bool,
}

impl<B: Bmc> CompatBmc<B> {
    /// Classifies the endpoint from its service root, one request, and
    /// returns the layer over `bmc`.
    ///
    /// # Errors
    ///
    /// The service root could not be read.
    pub async fn classify(bmc: Arc<B>) -> Result<Self, CompatError<B::Error>> {
        let root = NavProperty::<Raw>::new_reference(ODataId::service_root())
            .get(bmc.as_ref())
            .await
            .map_err(CompatError::Transport)?;
        let quirks = BmcQuirks::classify(&RootEvidence::from_root(root.value()));
        Ok(Self::new(bmc, Arc::new(quirks)))
    }

    /// The layer over `inner` for an already classified platform.
    #[must_use]
    pub fn new(inner: Arc<B>, quirks: Arc<BmcQuirks>) -> Self {
        let repairs = rules::any_enabled(&quirks);
        Self {
            inner,
            quirks,
            repairs,
        }
    }

    /// The transport underneath, for a request that must see the device's
    /// document as it came.
    #[must_use]
    pub const fn inner(&self) -> &Arc<B> {
        &self.inner
    }

    /// The platform this layer repairs for.
    #[must_use]
    pub const fn quirks(&self) -> &Arc<BmcQuirks> {
        &self.quirks
    }

    fn repaired<T>(&self, raw: Result<Arc<Raw>, B::Error>) -> Result<Arc<T>, CompatError<B::Error>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let raw = raw.map_err(CompatError::Transport)?;
        let target = if self.repairs {
            let mut document = raw.value().clone();
            rules::repair_in_place(&self.quirks, &mut document);
            serde_json::from_value(document)
        } else {
            T::deserialize(raw.value())
        };
        target.map(Arc::new).map_err(CompatError::Decode)
    }
}

// Cloning shares the transport and the classification; `B` need not be
// `Clone`.
impl<B: Bmc> Clone for CompatBmc<B> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            quirks: Arc::clone(&self.quirks),
            repairs: self.repairs,
        }
    }
}

impl<B> Bmc for CompatBmc<B>
where
    B: Bmc + 'static,
    B::Error: 'static,
{
    type Error = CompatError<B::Error>;

    async fn expand<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
    ) -> Result<Arc<T>, Self::Error> {
        self.repaired::<T>(self.inner.expand::<Raw>(id, query).await)
    }

    async fn get<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        self.repaired::<T>(self.inner.get::<Raw>(id).await)
    }

    async fn filter<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
    ) -> Result<Arc<T>, Self::Error> {
        self.repaired::<T>(self.inner.filter::<Raw>(id, query).await)
    }

    async fn create<V: Send + Sync + Serialize, R: Send + Sync + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner
            .create(id, query)
            .await
            .map_err(CompatError::Transport)
    }

    async fn create_session<
        V: Send + Sync + Serialize,
        R: Send + Sync + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<SessionCreateResponse<R>, Self::Error> {
        self.inner
            .create_session(id, query)
            .await
            .map_err(CompatError::Transport)
    }

    async fn update<
        V: Sync + Send + Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        etag: Option<&ODataETag>,
        update: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner
            .update(id, etag, update)
            .await
            .map_err(CompatError::Transport)
    }

    async fn delete<R: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner.delete(id).await.map_err(CompatError::Transport)
    }

    async fn action<
        T: Send + Sync + Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        action: &Action<T, R>,
        params: &T,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner
            .action(action, params)
            .await
            .map_err(CompatError::Transport)
    }

    async fn multipart_update<U, V, R>(
        &self,
        uri: &str,
        request: MultipartUpdateRequest<'_, U, V>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
        V: Send + Sync + Serialize,
    {
        self.inner
            .multipart_update(uri, request)
            .await
            .map_err(CompatError::Transport)
    }

    #[cfg(feature = "update-service-deprecated")]
    async fn http_push_uri_update<U, R>(
        &self,
        uri: &str,
        request: HttpPushUriUpdateRequest<U>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
    {
        self.inner
            .http_push_uri_update(uri, request)
            .await
            .map_err(CompatError::Transport)
    }

    async fn stream<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
    ) -> Result<BoxTryStream<T, Self::Error>, Self::Error> {
        let stream = self
            .inner
            .stream::<T>(uri)
            .await
            .map_err(CompatError::Transport)?;
        let mapped: BoxTryStream<T, Self::Error> = Box::pin(stream.map_err(CompatError::Transport));
        Ok(mapped)
    }

    async fn stream_events<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> Result<BoxTryStream<StreamEvent<T>, Self::Error>, Self::Error> {
        let stream = self
            .inner
            .stream_events::<T>(uri, last_event_id)
            .await
            .map_err(CompatError::Transport)?;
        let mapped: BoxTryStream<StreamEvent<T>, Self::Error> =
            Box::pin(stream.map_err(CompatError::Transport));
        Ok(mapped)
    }
}
