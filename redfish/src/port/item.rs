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

use crate::entity_link::EntityLink;
use crate::entity_link::FromLink;
use crate::mac_address::MacAddress;
use crate::schema::port::Port as PortSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "oem-lenovo")]
use crate::oem::lenovo::port::LenovoPort;

/// Link for lazily accessing a network port.
pub type PortLink<B> = EntityLink<B, PortSchema>;

/// Network port.
pub struct Port<B: Bmc> {
    data: Arc<PortSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> Port<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<PortSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this port.
    #[must_use]
    pub fn raw(&self) -> Arc<PortSchema> {
        self.data.clone()
    }

    /// Get the standard Ethernet MAC addresses associated with this port.
    ///
    /// Returns the values reported in `Ethernet.AssociatedMACAddresses`.
    #[must_use]
    pub fn associated_mac_addresses(&self) -> Vec<MacAddress<'_>> {
        self.data
            .ethernet
            .as_ref()
            .and_then(Option::as_ref)
            .and_then(|ethernet| ethernet.associated_mac_addresses.as_ref())
            .and_then(Option::as_ref)
            .map(|addresses| {
                addresses
                    .iter()
                    .map(String::as_str)
                    .map(MacAddress::new)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get Lenovo Port OEM data.
    ///
    /// Returns `Ok(None)` when the port does not include `Oem.Lenovo`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing Lenovo port OEM data fails.
    #[cfg(feature = "oem-lenovo")]
    pub fn oem_lenovo(&self) -> Result<Option<LenovoPort>, Error<B>> {
        LenovoPort::new(&self.data).map_err(Error::Json)
    }
}

impl<B: Bmc> FromLink<B> for Port<B> {
    type Schema = PortSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}
