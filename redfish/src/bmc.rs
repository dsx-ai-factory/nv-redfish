// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! BMC implementaion that takes in account protocol features.  That
//! is built on top of core BMC.

use crate::bmc_quirks::BmcQuirks;
use crate::protocol_features::ExpandQueryFeatures;
use crate::Error;
use crate::ProtocolFeatures;
use nv_redfish_core::Bmc;
use nv_redfish_core::BmcError as _;
use nv_redfish_core::BmcErrorClass;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use std::sync::Arc;

#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::query::ExpandQuery;
#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::Expandable;

pub struct NvBmc<B: Bmc> {
    bmc: Arc<B>,
    protocol_features: Arc<ProtocolFeatures>,
    pub(crate) quirks: Arc<BmcQuirks>,
}

impl<B: Bmc> NvBmc<B> {
    pub(crate) fn new(bmc: Arc<B>, protocol_features: ProtocolFeatures, quirks: BmcQuirks) -> Self {
        Self {
            bmc,
            protocol_features: protocol_features.into(),
            quirks: quirks.into(),
        }
    }

    pub(crate) fn replace_bmc(self, bmc: Arc<B>) -> Self {
        Self {
            bmc,
            protocol_features: self.protocol_features,
            quirks: self.quirks,
        }
    }

    pub(crate) fn restrict_expand(self) -> Self {
        Self {
            bmc: self.bmc,
            protocol_features: ProtocolFeatures {
                expand: ExpandQueryFeatures {
                    expand_all: false,
                    no_links: false,
                },
            }
            .into(),
            quirks: self.quirks,
        }
    }

    #[allow(dead_code)] // feature-enabled func
    pub fn as_ref(&self) -> &B {
        self.bmc.as_ref()
    }

    /// Expand navigation property with optimal available method.
    ///
    /// # Errors
    ///
    /// Returns `Error::Bmc` if failed to send request to the BMC.
    ///
    #[cfg(feature = "impl-nv-bmc-expand")]
    pub async fn expand_property<T>(&self, nav: &NavProperty<T>) -> Result<Arc<T>, Error<B>>
    where
        T: Expandable,
    {
        let optimal_query = if self.protocol_features.expand.no_links {
            // Prefer no links expand.
            Some(ExpandQuery::no_links())
        } else if self.protocol_features.expand.expand_all {
            Some(ExpandQuery::all())
        } else {
            None
        };
        if let Some(optimal_query) = optimal_query {
            nav.expand(self.bmc.as_ref(), optimal_query)
                .await
                .map_err(Error::Bmc)?
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)
        } else {
            // if query is not suported.
            nav.get(self.bmc.as_ref()).await.map_err(Error::Bmc)
        }
    }

    /// Fetch `id` and deserialize the response body into `T`.
    ///
    /// For resources no schema describes as an entity; the body may omit
    /// `@odata.id`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Bmc` if fetching the resource fails, or `Error::Json`
    /// if the body does not deserialize into `T`.
    #[allow(dead_code)] // feature-enabled func
    pub async fn get_raw<T: DeserializeOwned>(&self, id: &ODataId) -> Result<T, Error<B>> {
        let raw = NavProperty::<RawBody>::new_reference(id.clone())
            .get(self.bmc.as_ref())
            .await
            .map_err(Error::Bmc)?;
        serde_json::from_value(raw.body.clone()).map_err(Error::Json)
    }

    /// Update the resource at `id`, sending `etag` as `If-Match`.
    ///
    /// On BMCs that reject their own ETags the ETag is dropped, so the
    /// transport sends `If-Match: *`. On BMCs that accept a staged change
    /// with a `202` naming no task, that `202` reads as an empty response.
    ///
    /// # Errors
    ///
    /// Returns `Error::Bmc` if the update fails.
    #[allow(dead_code)] // feature-enabled func
    pub(crate) async fn update<V, R>(
        &self,
        id: &ODataId,
        etag: Option<&ODataETag>,
        update: &V,
    ) -> Result<ModificationResponse<R>, Error<B>>
    where
        V: Sync + Send + Serialize,
        R: Send + Sync + for<'de> Deserialize<'de>,
    {
        let etag = if self.quirks.bug_rejects_own_etag() {
            None
        } else {
            etag
        };
        match self.bmc.update(id, etag, update).await {
            Err(error)
                if self.quirks.bug_accepted_update_without_task()
                    && error.error_class() == BmcErrorClass::HttpResponse { status: 202 } =>
            {
                Ok(ModificationResponse::Empty)
            }
            result => result.map_err(Error::Bmc),
        }
    }
}

// Implementing Clone because derive requires B to be Clone but NvBmc
// doesn't require it.
impl<B: Bmc> Clone for NvBmc<B> {
    fn clone(&self) -> Self {
        Self {
            bmc: self.bmc.clone(),
            protocol_features: self.protocol_features.clone(),
            quirks: self.quirks.clone(),
        }
    }
}

/// A response body kept as it arrived, for [`NvBmc::get_raw`].
struct RawBody {
    id: ODataId,
    body: serde_json::Value,
}

impl EntityTypeRef for RawBody {
    fn odata_id(&self) -> &ODataId {
        &self.id
    }
    fn etag(&self) -> Option<&ODataETag> {
        None
    }
}

impl<'de> Deserialize<'de> for RawBody {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self {
            id: String::new().into(),
            body: serde_json::Value::deserialize(deserializer)?,
        })
    }
}
