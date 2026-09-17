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

//! Ethernet interfaces
//!

use crate::mac_address::MacAddress;
use crate::schema::ethernet_interface::EthernetInterface as EthernetInterfaceSchema;
use crate::schema::ethernet_interface_collection::EthernetInterfaceCollection as EthernetInterfaceCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;
use tagged_types::TaggedType;

#[doc(inline)]
pub use crate::schema::ethernet_interface::{EthernetInterfaceUpdate, LinkStatus};

/// Ethernet interfaces collection.
///
/// Provides functions to access collection members.
pub struct EthernetInterfaceCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<EthernetInterfaceCollectionSchema>,
}

impl<B: Bmc> EthernetInterfaceCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<EthernetInterfaceCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = bmc.expand_property(nav).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List all managers available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching manager data fails.
    pub async fn members(&self) -> Result<Vec<EthernetInterface<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(EthernetInterface::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

/// Uefi device path for the interface.
///
/// Nv-redfish keeps open underlying type for `UefiDevicePath` because it
/// can really be represented by any implementation of UEFI's device path.
pub type UefiDevicePath<T> = TaggedType<T, UefiDevicePathTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, FromStr, Serialize, Deserialize)]
#[capability(inner_access)]
pub enum UefiDevicePathTag {}

/// Ethernet Interface.
///
/// Provides functions to access ethernet interface.
pub struct EthernetInterface<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<EthernetInterfaceSchema>,
}

impl<B: Bmc> EthernetInterface<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<EthernetInterfaceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this ethernet interface.
    #[must_use]
    pub fn raw(&self) -> Arc<EthernetInterfaceSchema> {
        self.data.clone()
    }

    /// Update this ethernet interface.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &EthernetInterfaceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<EthernetInterfaceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// State of the interface. `None` means that BMC hasn't reported
    /// interface state or reported null.
    #[must_use]
    pub fn interface_enabled(&self) -> Option<bool> {
        self.data
            .interface_enabled
            .as_ref()
            .and_then(Option::as_ref)
            .copied()
    }

    /// Link status of the interface.
    #[must_use]
    pub fn link_status(&self) -> Option<LinkStatus> {
        self.data
            .link_status
            .as_ref()
            .and_then(Option::as_ref)
            .copied()
    }

    /// MAC address of the interface.
    #[must_use]
    pub fn mac_address(&self) -> Option<MacAddress<'_>> {
        self.data
            .mac_address
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(MacAddress::new)
    }

    /// Permanent MAC address of the interface.
    #[must_use]
    pub fn permanent_mac_address(&self) -> Option<MacAddress<'_>> {
        self.data
            .permanent_mac_address
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(MacAddress::new)
    }

    /// UEFI device path for the interface.
    #[must_use]
    pub fn uefi_device_path(&self) -> Option<UefiDevicePath<&str>> {
        self.data
            .uefi_device_path
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(UefiDevicePath::new)
    }
}
