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

//! Support HPE Manager OEM extension.

use crate::oem::hpe::schema::hpei_lo::HpeiLo as HpeManagerSchema;
use crate::oem::oem_object;
use crate::schema::manager::Manager as ManagerSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use std::sync::Arc;

/// Represents an HPE OEM extension to Manager schema.
pub struct HpeManager<B: Bmc> {
    data: Arc<HpeManagerSchema>,
    _bmc: NvBmc<B>,
}

impl<B: Bmc> HpeManager<B> {
    /// Create a new manager OEM wrapper.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.Hpe`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing HPE manager OEM data fails.
    pub(crate) fn new(bmc: &NvBmc<B>, manager: &ManagerSchema) -> Result<Option<Self>, Error<B>> {
        Ok(manager
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Hpe"))?
            .map(|data| Self {
                data,
                _bmc: bmc.clone(),
            }))
    }

    /// Get the raw schema data for this HPE Manager.
    #[must_use]
    pub fn raw(&self) -> Arc<HpeManagerSchema> {
        self.data.clone()
    }

    /// Host-side virtual NIC support state.
    #[must_use]
    pub fn virtual_nic_enabled(&self) -> Option<bool> {
        self.data.virtual_nic_enabled
    }
}
