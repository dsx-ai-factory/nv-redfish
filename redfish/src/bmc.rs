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
use crate::ProtocolFeatures;
use nv_redfish_core::Bmc;
use nv_redfish_quirks::BmcQuirks;
use std::sync::Arc;

#[cfg(feature = "impl-nv-bmc-expand")]
use crate::Error;
#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::query::ExpandQuery;
#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::Expandable;
#[cfg(feature = "impl-nv-bmc-expand")]
use nv_redfish_core::NavProperty;

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

    /// The transport, shared.
    pub(crate) fn shared(&self) -> Arc<B> {
        Arc::clone(&self.bmc)
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
