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

use crate::protocol_features::ExpandQueryFeatures;
use crate::Error;
use crate::ProtocolFeatures;
use nv_redfish_core::repair::decode;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::Raw;
use nv_redfish_quirks::BmcQuirks;
use nv_redfish_quirks::CompatBmc;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;

#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::query::ExpandQuery;
#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::Expandable;

pub struct NvBmc<B: Bmc> {
    /// Every request goes through the compatibility layer, so the wrappers
    /// get the platform's and the caller's repairs.
    bmc: CompatBmc<B>,
    protocol_features: Arc<ProtocolFeatures>,
    pub(crate) quirks: Arc<BmcQuirks>,
}

impl<B: Bmc> NvBmc<B> {
    pub(crate) fn new(bmc: CompatBmc<B>, protocol_features: ProtocolFeatures) -> Self {
        Self {
            quirks: Arc::clone(bmc.quirks()),
            bmc,
            protocol_features: protocol_features.into(),
        }
    }

    pub(crate) fn replace_bmc(self, bmc: Arc<B>) -> Self {
        Self {
            bmc: self.bmc.replace_inner(bmc),
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
    pub const fn as_ref(&self) -> &CompatBmc<B> {
        &self.bmc
    }

    /// The compatibility layer.
    pub(crate) fn compat(&self) -> CompatBmc<B> {
        self.bmc.clone()
    }

    /// Expand navigation property with optimal available method.
    ///
    /// # Errors
    ///
    /// Returns `Error::Bmc` if failed to send request to the BMC, or
    /// `Error::Decode` if the repaired document did not deserialize.
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
            nav.expand(&self.bmc, optimal_query)
                .await
                .map_err(Error::from)?
                .get(&self.bmc)
                .await
                .map_err(Error::from)
        } else {
            // if query is not suported.
            nav.get(&self.bmc).await.map_err(Error::from)
        }
    }

    /// Fetch `id` and deserialize the response body into `T`.
    ///
    /// For resources no schema describes as an entity; the body may omit
    /// `@odata.id`.
    ///
    /// # Errors
    ///
    /// Returns `Error::Bmc` if fetching the resource fails, or
    /// `Error::Decode` if the body does not deserialize into `T`.
    #[allow(dead_code)] // feature-enabled func
    pub async fn get_raw<T: DeserializeOwned>(&self, id: &ODataId) -> Result<T, Error<B>> {
        let raw = NavProperty::<Raw>::new_reference(id.clone())
            .get(&self.bmc)
            .await
            .map_err(Error::from)?;
        let body = Arc::try_unwrap(raw).map_or_else(|raw| raw.value().clone(), Raw::into_value);
        decode(body).map_err(Error::Decode)
    }

    /// Update the resource at `id`, sending `etag` as `If-Match`.
    ///
    /// The platform's protocol quirks apply in the compatibility layer: on
    /// BMCs that reject their own ETags the transport sends `If-Match: *`,
    /// and on BMCs that accept a staged change with a `202` naming no task,
    /// that `202` reads as an empty response.
    ///
    /// # Errors
    ///
    /// Returns `Error::Bmc` if the update fails, or `Error::Decode` if the
    /// reply does not deserialize into `R`.
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
        self.bmc.update(id, etag, update).await.map_err(Error::from)
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
