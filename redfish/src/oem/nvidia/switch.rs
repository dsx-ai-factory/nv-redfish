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

//! NVIDIA Switch OEM extension.

use crate::oem::nvidia::schema::nvidia_switch::FabricManager as FabricManagerSchema;
use crate::oem::nvidia::schema::nvidia_switch::NvidiaSwitch as NvidiaSwitchSchema;
use crate::oem::nvidia::OEM_KEY;
use crate::oem::oem_object;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;
use nv_redfish_core::Bmc;
use std::convert::identity;
use std::marker::PhantomData;
use std::sync::Arc;
use tagged_types::TaggedType;

#[doc(inline)]
pub use crate::oem::nvidia::schema::nvidia_manager::FabricManagerState;
#[doc(inline)]
pub use crate::oem::nvidia::schema::nvidia_manager::ReportStatus as FabricManagerReportStatus;
#[doc(inline)]
pub use crate::oem::nvidia::schema::nvidia_switch::SwitchIsolationMode;

/// PCI device ID of the switch.
pub type DeviceId<T> = TaggedType<T, DeviceIdTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum DeviceIdTag {}

/// PCI vendor ID of the switch.
pub type VendorId<T> = TaggedType<T, VendorIdTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum VendorIdTag {}

/// NVIDIA OEM extension of a Redfish `Switch`.
pub struct NvidiaSwitch<B: Bmc> {
    data: Arc<NvidiaSwitchSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> NvidiaSwitch<B> {
    /// Read the extension out of a `Switch` OEM payload.
    ///
    /// Returns `Ok(None)` when the payload carries no NVIDIA object,
    /// including when it carries an explicit `null`.
    pub(crate) fn new(oem: &ResourceOemSchema) -> Result<Option<Self>, Error<B>> {
        Ok(oem_object(oem, OEM_KEY)?.map(|data| Self {
            data,
            _marker: PhantomData,
        }))
    }

    /// Get the raw schema data for this NVIDIA switch extension.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaSwitchSchema> {
        self.data.clone()
    }

    /// PCI device ID of the switch.
    #[must_use]
    pub fn device_id(&self) -> Option<DeviceId<&str>> {
        self.data
            .device_id
            .as_ref()
            .and_then(Option::as_deref)
            .map(DeviceId::new)
    }

    /// PCI vendor ID of the switch.
    #[must_use]
    pub fn vendor_id(&self) -> Option<VendorId<&str>> {
        self.data
            .vendor_id
            .as_ref()
            .and_then(Option::as_deref)
            .map(VendorId::new)
    }

    /// Whether the PCIe reference clock is enabled.
    #[must_use]
    pub fn pcie_reference_clock_enabled(&self) -> Option<bool> {
        self.data.pcie_reference_clock_enabled.and_then(identity)
    }

    /// Whether protected PCIe mode is enabled.
    #[must_use]
    pub fn ppcie_mode_enabled(&self) -> Option<bool> {
        self.data.ppcie_mode_enabled.and_then(identity)
    }

    /// Whether the switch may communicate with the rest of the fabric.
    #[must_use]
    pub fn switch_isolation_mode(&self) -> Option<SwitchIsolationMode> {
        self.data.switch_isolation_mode.and_then(identity)
    }

    /// State of the fabric manager as observed from this switch.
    #[must_use]
    pub fn fabric_manager(&self) -> Option<&FabricManagerSchema> {
        self.data.fabric_manager.as_ref().and_then(Option::as_ref)
    }

    /// State of the fabric manager as observed from this switch.
    #[must_use]
    pub fn fabric_manager_state(&self) -> Option<FabricManagerState> {
        self.fabric_manager()
            .and_then(|fm| fm.state)
            .and_then(identity)
    }
}
