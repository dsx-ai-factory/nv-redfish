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

use super::Protocol;
use crate::entity_link::EntityLink;
use crate::entity_link::FromLink;
use crate::schema::fabric::Fabric as FabricSchema;
use crate::Error;
use crate::NvBmc;
use crate::ResourceProvidesStatus;
use crate::ResourceStatusSchema;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::future::Future;
use std::sync::Arc;

#[cfg(feature = "switches")]
use super::SwitchCollection;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaFabric;

/// Lazy link to a fabric.
pub type FabricLink<B> = EntityLink<B, FabricSchema>;

/// Represents a fabric in the system.
///
/// Provides access to fabric information and the switches it contains.
pub struct Fabric<B: Bmc> {
    #[allow(dead_code)] // Used when switches are enabled.
    bmc: NvBmc<B>,
    data: Arc<FabricSchema>,
}

impl<B: Bmc> Fabric<B> {
    /// Create a new fabric handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<FabricSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this fabric.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<FabricSchema> {
        self.data.clone()
    }

    /// The protocol being sent over this fabric.
    #[must_use]
    pub fn fabric_type(&self) -> Option<Protocol> {
        self.data.fabric_type.and_then(identity)
    }

    /// The maximum number of zones the fabric can support.
    #[must_use]
    pub fn max_zones(&self) -> Option<i64> {
        self.data.max_zones.and_then(identity)
    }

    /// Get switches of this fabric.
    ///
    /// Returns `Ok(None)` when the switches link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the switch collection fails.
    #[cfg(feature = "switches")]
    pub async fn switches(&self) -> Result<Option<SwitchCollection<B>>, Error<B>> {
        if let Some(switches_ref) = &self.data.switches {
            SwitchCollection::new(&self.bmc, switches_ref)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get NVIDIA Fabric OEM extension.
    ///
    /// Returns `Ok(None)` when the fabric does not include `Oem.Nvidia`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing NVIDIA fabric OEM data fails.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia(&self) -> Result<Option<NvidiaFabric<B>>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), NvidiaFabric::new)
    }
}

impl<B: Bmc> ResourceProvidesStatus for Fabric<B> {
    fn resource_status_ref(&self) -> Option<&ResourceStatusSchema> {
        self.data.status.as_ref()
    }
}

impl<B: Bmc> FromLink<B> for Fabric<B> {
    type Schema = FabricSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}
