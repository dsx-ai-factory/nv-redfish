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
//! [`CompatBmc`] applies the repairs enabled for the platform to every
//! document it reads — members a device expanded inline included, at any
//! depth — before the document is deserialized into the type the caller
//! asked for. It hands the repairs to the transport through
//! [`Bmc::get_repaired`] and its siblings, so a transport that deserializes
//! documents itself repairs in the same pass, and a
//! [`RulesBmc`](crate::RulesBmc) underneath adds the caller's rules after
//! them. It is classified once, from the service root, with one request.
//!
//! What a write answers with is the transport's: creates, updates,
//! deletes, polls, actions and uploads go through as they are, so the
//! transport's own reading of a reply still applies. The layer adds only
//! the platform's protocol quirks for an update.
//!
//! Each payload of an event stream is repaired as it arrives: first the
//! event repairs, which may take the SSE id the payload arrived under, then
//! the document repairs. A payload that still does not deserialize is one
//! error in the stream, not its end.
//!
//! A caching transport keeps a document as it was repaired and serves it to
//! every later read of the resource. Every layer over one transport repairs
//! the same way, since the transport serves one device and the device
//! classifies the same way each time, so the layers share its cache. A read
//! that goes around the layer shares that cache too: read a resource either
//! through the layer or around it.

use std::sync::Arc;

use futures_util::StreamExt as _;
use nv_redfish_core::query::ExpandQuery;
use nv_redfish_core::repair::decode;
use nv_redfish_core::repair::decode_raw;
use nv_redfish_core::Action;
use nv_redfish_core::Bmc;
use nv_redfish_core::BmcError as _;
use nv_redfish_core::BmcErrorClass;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
use nv_redfish_core::FilterQuery;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::MultipartUpdateRequest;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::Raw;
use nv_redfish_core::Repair;
use nv_redfish_core::RepairError;
use nv_redfish_core::SessionCreateResponse;
use nv_redfish_core::StreamEvent;
use nv_redfish_core::UploadReader;
use serde::Deserialize;
use serde::Serialize;

use crate::events::EventRepairs;
use crate::rules;
use crate::rules::Enabled;
use crate::BmcQuirks;
use crate::DecodeError;
use crate::RootEvidence;

/// What a request through [`CompatBmc`] can fail with.
pub type CompatError<E> = RepairError<E>;

/// The repairs for the payloads of one event stream.
struct StreamRepairs {
    events: EventRepairs,
    enabled: Enabled,
}

impl StreamRepairs {
    fn repair(&self, payload: &mut serde_json::Value, last_event_id: Option<&str>) {
        self.events.repair(payload, last_event_id);
        self.enabled.repair(payload);
    }
}

/// A [`Bmc`] that repairs what it reads.
///
/// Repairs are the classified platform's; everything else is delegated to
/// the transport underneath.
pub struct CompatBmc<B: Bmc> {
    inner: Arc<B>,
    quirks: Arc<BmcQuirks>,
    /// The platform's enabled repairs.
    enabled: Enabled,
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
            .map_err(CompatError::from_transport)?;
        let quirks = BmcQuirks::classify(&RootEvidence::from_root(root.value()));
        Ok(Self::new(bmc, Arc::new(quirks)))
    }

    /// The layer over `inner` for an already classified platform.
    #[must_use]
    pub fn new(inner: Arc<B>, quirks: Arc<BmcQuirks>) -> Self {
        Self {
            inner,
            enabled: rules::enabled(&quirks),
            quirks,
        }
    }

    /// The transport underneath, for a request that must see the device's
    /// document as it came.
    #[must_use]
    pub const fn inner(&self) -> &Arc<B> {
        &self.inner
    }

    /// The same layer over another transport for the same endpoint.
    #[must_use]
    pub fn replace_inner(&self, inner: Arc<B>) -> Self {
        Self {
            inner,
            quirks: Arc::clone(&self.quirks),
            enabled: self.enabled.clone(),
        }
    }

    /// The platform this layer repairs for.
    #[must_use]
    pub const fn quirks(&self) -> &Arc<BmcQuirks> {
        &self.quirks
    }

    /// The repairs for the payloads of one event stream.
    fn stream_repairs(&self) -> StreamRepairs {
        StreamRepairs {
            events: EventRepairs::new(&self.quirks),
            enabled: self.enabled.clone(),
        }
    }

    /// Repairs a document obtained some other way, such as a write's reply,
    /// with the platform's repairs, and deserializes it into `T`. A
    /// [`RulesBmc`](crate::RulesBmc) underneath applies the caller's rules
    /// to what it reads, so a document it did not read does not get them.
    ///
    /// # Errors
    ///
    /// The repaired document does not deserialize into `T`.
    pub fn decode<T>(&self, mut document: serde_json::Value) -> Result<T, DecodeError>
    where
        T: for<'de> Deserialize<'de>,
    {
        self.enabled.repair(&mut document);
        decode(document)
    }
}

// Cloning shares the transport and the classification; `B` need not be
// `Clone`.
impl<B: Bmc> Clone for CompatBmc<B> {
    fn clone(&self) -> Self {
        self.replace_inner(Arc::clone(&self.inner))
    }
}

impl<B: Bmc> Bmc for CompatBmc<B> {
    type Error = CompatError<B::Error>;

    async fn expand<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
    ) -> Result<Arc<T>, Self::Error> {
        self.inner.expand_repaired(id, query, &self.enabled).await
    }

    async fn get<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        self.inner.get_repaired(id, &self.enabled).await
    }

    async fn filter<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
    ) -> Result<Arc<T>, Self::Error> {
        self.inner.filter_repaired(id, query, &self.enabled).await
    }

    // Another repair on top of the layer's: the document is read through
    // the layer and repaired again.
    async fn get_repaired<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        repair: &impl Repair,
    ) -> Result<Arc<T>, RepairError<Self::Error>> {
        decode_raw(self.get::<Raw>(id).await, repair)
    }

    async fn expand_repaired<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
        repair: &impl Repair,
    ) -> Result<Arc<T>, RepairError<Self::Error>> {
        decode_raw(self.expand::<Raw>(id, query).await, repair)
    }

    async fn filter_repaired<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
        repair: &impl Repair,
    ) -> Result<Arc<T>, RepairError<Self::Error>> {
        decode_raw(self.filter::<Raw>(id, query).await, repair)
    }

    async fn create<V: Send + Sync + Serialize, R: Send + Sync + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner
            .create(id, query)
            .await
            .map_err(CompatError::from_transport)
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
            .map_err(CompatError::from_transport)
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
        // Without an ETag the transport sends `If-Match: *`, the only one
        // such firmware accepts.
        let etag = if self.quirks.bug_rejects_own_etag() {
            None
        } else {
            etag
        };
        match self.inner.update(id, etag, update).await {
            Err(error)
                if self.quirks.bug_accepted_update_without_task()
                    && error.error_class() == BmcErrorClass::HttpResponse { status: 202 } =>
            {
                Ok(ModificationResponse::Empty)
            }
            result => result.map_err(CompatError::from_transport),
        }
    }

    async fn delete<R: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner
            .delete(id)
            .await
            .map_err(CompatError::from_transport)
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
            .map_err(CompatError::from_transport)
    }

    async fn poll<R: Send + Sync + Sized + for<'de> Deserialize<'de>>(
        &self,
        uri: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner
            .poll(uri)
            .await
            .map_err(CompatError::from_transport)
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
            .map_err(CompatError::from_transport)
    }

    nv_redfish_core::if_update_service_deprecated! {
        async fn http_push_uri_update<U, R>(
            &self,
            uri: &str,
            request: nv_redfish_core::HttpPushUriUpdateRequest<U>,
        ) -> Result<ModificationResponse<R>, Self::Error>
        where
            U: UploadReader,
            R: Send + Sync + for<'de> Deserialize<'de>,
        {
            self.inner
                .http_push_uri_update(uri, request)
                .await
                .map_err(CompatError::from_transport)
        }
    }

    async fn stream<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
    ) -> Result<BoxTryStream<T, Self::Error>, Self::Error> {
        let stream = self
            .inner
            .stream::<serde_json::Value>(uri)
            .await
            .map_err(CompatError::from_transport)?;
        let repairs = self.stream_repairs();
        let mapped: BoxTryStream<T, Self::Error> = Box::pin(stream.map(move |item| {
            let mut payload = item.map_err(CompatError::from_transport)?;
            repairs.repair(&mut payload, None);
            decode(payload).map_err(CompatError::Decode)
        }));
        Ok(mapped)
    }

    async fn stream_events<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> Result<BoxTryStream<StreamEvent<T>, Self::Error>, Self::Error> {
        let stream = self
            .inner
            .stream_events::<serde_json::Value>(uri, last_event_id)
            .await
            .map_err(CompatError::from_transport)?;
        let repairs = self.stream_repairs();
        let mapped: BoxTryStream<StreamEvent<T>, Self::Error> = Box::pin(stream.map(move |item| {
            let StreamEvent {
                last_event_id,
                data: mut payload,
            } = item.map_err(CompatError::from_transport)?;
            // The stream's id is only read: an inherited one stays as it is.
            repairs.repair(&mut payload, last_event_id.as_deref());
            decode(payload)
                .map(|data| StreamEvent {
                    last_event_id,
                    data,
                })
                .map_err(CompatError::Decode)
        }));
        Ok(mapped)
    }
}
