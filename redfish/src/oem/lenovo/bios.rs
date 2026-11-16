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

//! Lenovo OEM extension to a BIOS resource.

use std::sync::Arc;

use nv_redfish_core::Bmc;

use crate::oem::lenovo::oem_update;
use crate::oem::lenovo::schema::lenovo_bios::LenovoBios as LenovoBiosSchema;
use crate::oem::oem_object;
use crate::schema::bios::BiosUpdate;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_bios::LenovoBiosUpdate;

/// Lenovo OEM extension to a BIOS resource, advertised as `Bios.Oem.Lenovo`.
pub struct LenovoBios {
    data: Arc<LenovoBiosSchema>,
}

impl LenovoBios {
    /// Parse the extension advertised under `Bios.Oem.Lenovo`.
    ///
    /// Returns `Ok(None)` when the BIOS does not include `Oem.Lenovo`.
    pub(crate) fn new<B: Bmc>(oem: &ResourceOemSchema) -> Result<Option<Self>, Error<B>> {
        Ok(oem_object(oem, "Lenovo")?.map(|data| Self { data }))
    }

    /// Get the raw Lenovo BIOS extension.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoBiosSchema> {
        self.data.clone()
    }

    /// Whether UEFI settings changes are prohibited (`SettingsLockdown`).
    ///
    /// Returns `None` when the firmware does not report the control.
    #[must_use]
    pub fn settings_lockdown(&self) -> Option<bool> {
        self.data.settings_lockdown
    }

    /// Whether UEFI variables are read-only (`UEFIVariablesRO`).
    ///
    /// Returns `None` when the firmware does not report the control.
    #[must_use]
    pub fn uefi_variables_ro(&self) -> Option<bool> {
        self.data.uefi_variables_ro
    }
}

/// Adds Lenovo OEM settings to a standard BIOS update.
///
/// Lenovo applies these settings to the current BIOS resource rather than its
/// settings object, so pass the result to `Bios::update` on the handle from
/// `ComputerSystem::bios`:
///
/// ```text
/// PATCH /redfish/v1/Systems/{id}/Bios
/// {"Oem": {"Lenovo": {"SettingsLockdown": true}}}
/// ```
pub trait LenovoBiosUpdateExt: Sized {
    /// Merge Lenovo OEM settings while preserving other OEM values.
    ///
    /// # Errors
    ///
    /// Returns an error if the Lenovo update cannot be serialized.
    fn with_oem_lenovo(self, lenovo_update: LenovoBiosUpdate) -> Result<Self, serde_json::Error>;
}

impl LenovoBiosUpdateExt for BiosUpdate {
    fn with_oem_lenovo(
        mut self,
        lenovo_update: LenovoBiosUpdate,
    ) -> Result<Self, serde_json::Error> {
        self.oem = Some(oem_update(self.oem.take(), &lenovo_update)?);
        Ok(self)
    }
}
