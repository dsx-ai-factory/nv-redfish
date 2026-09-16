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

//! PCIe devices
//!

use crate::hardware_id::HardwareIdRef;
use crate::hardware_id::Manufacturer as HardwareIdManufacturer;
use crate::hardware_id::Model as HardwareIdModel;
use crate::hardware_id::PartNumber as HardwareIdPartNumber;
use crate::hardware_id::SerialNumber as HardwareIdSerialNumber;
use crate::schema::pcie_device::PcieDevice as PcieDeviceSchema;
#[cfg(feature = "chassis")]
use crate::schema::pcie_device_collection::PcieDeviceCollection as PcieDeviceCollectionSchema;
#[cfg(feature = "chassis")]
use crate::Error;
#[cfg(feature = "chassis")]
use crate::NvBmc;
use crate::ResourceProvidesStatus;
use crate::ResourceStatusSchema;
use nv_redfish_core::Bmc;
#[cfg(feature = "chassis")]
use nv_redfish_core::NavProperty;
use std::marker::PhantomData;
use std::sync::Arc;
use tagged_types::TaggedType;

/// PCIe devices collection.
///
/// Provides functions to access collection members.
#[cfg(feature = "chassis")]
pub struct PcieDeviceCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<PcieDeviceCollectionSchema>,
}

#[cfg(feature = "chassis")]
impl<B: Bmc> PcieDeviceCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<PcieDeviceCollectionSchema>,
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
    pub async fn members(&self) -> Result<Vec<PcieDevice<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(PcieDevice::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

#[doc(hidden)]
pub enum PcieDeviceTag {}

/// PCIe device manufacturer.
pub type Manufacturer<T> = HardwareIdManufacturer<T, PcieDeviceTag>;

/// PCIe device model.
pub type Model<T> = HardwareIdModel<T, PcieDeviceTag>;

/// PCIe device part number.
pub type PartNumber<T> = HardwareIdPartNumber<T, PcieDeviceTag>;

/// PCIe device serial number.
pub type SerialNumber<T> = HardwareIdSerialNumber<T, PcieDeviceTag>;

/// Firmware version of the PCIe device.
pub type FirmwareVersion<T> = TaggedType<T, FirmwareVersionTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum FirmwareVersionTag {}

/// PCIe device.
///
/// Provides functions to access PCIe device data.
pub struct PcieDevice<B: Bmc> {
    data: Arc<PcieDeviceSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> PcieDevice<B> {
    /// Create a new log service handle.
    #[cfg(feature = "chassis")]
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<PcieDeviceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this PCIe device.
    #[must_use]
    pub fn raw(&self) -> Arc<PcieDeviceSchema> {
        self.data.clone()
    }

    /// Get hardware identifier of the PCIe device.
    #[must_use]
    pub fn hardware_id(&self) -> HardwareIdRef<'_, PcieDeviceTag> {
        HardwareIdRef {
            manufacturer: self
                .data
                .manufacturer
                .as_ref()
                .and_then(Option::as_deref)
                .map(Manufacturer::new),
            model: self
                .data
                .model
                .as_ref()
                .and_then(Option::as_deref)
                .map(Model::new),
            part_number: self
                .data
                .part_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(PartNumber::new),
            serial_number: self
                .data
                .serial_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(SerialNumber::new),
        }
    }

    /// The version of firmware for this PCIe device.
    #[must_use]
    pub fn firmware_version(&self) -> Option<FirmwareVersion<&str>> {
        self.data
            .firmware_version
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(FirmwareVersion::new)
    }
}

impl<B: Bmc> ResourceProvidesStatus for PcieDevice<B> {
    fn resource_status_ref(&self) -> Option<&ResourceStatusSchema> {
        self.data.status.as_ref()
    }
}
