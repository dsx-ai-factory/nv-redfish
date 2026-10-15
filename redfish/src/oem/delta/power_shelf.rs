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

//! Support Delta Energy Systems power shelf OEM extension.

use crate::core::ActionError;
use crate::core::ModificationResponse;
use crate::oem::delta::schema::delta_energy_systems_power_distribution::Actions as DeltaPowerShelfActions;
use crate::oem::delta::schema::delta_energy_systems_power_distribution::PowerDistribution as DeltaPowerShelfSchema;
use crate::oem::delta::OEM_KEY;
use crate::oem::oem_object;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use std::convert::identity;
use std::sync::Arc;

/// Delta Energy Systems OEM extension for a power shelf.
///
/// Delta power shelves have no `ComputerSystem`; their PSUs are switched on
/// and off through OEM actions under `Oem/deltaenergysystems` on the
/// `PowerDistribution` resource.
pub struct DeltaPowerShelf<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<DeltaPowerShelfSchema>,
}

impl<B: Bmc> DeltaPowerShelf<B> {
    /// Create a Delta OEM power shelf handle from a power shelf's `Oem` bag.
    ///
    /// Returns `Ok(None)` when the OEM payload does not contain Delta power
    /// shelf data.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing the Delta OEM data fails.
    pub(crate) fn new(bmc: &NvBmc<B>, oem: &ResourceOemSchema) -> Result<Option<Self>, Error<B>> {
        Ok(oem_object(oem, OEM_KEY)?.map(|data| Self {
            bmc: bmc.clone(),
            data,
        }))
    }

    /// Turn on all PSUs in the shelf.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the shelf omits its Actions
    /// container, or a BMC error if the action is not advertised or
    /// invocation fails.
    pub async fn turn_on_psus(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        self.actions()?
            .turn_on_psus(self.bmc.as_ref())
            .await
            .map_err(Error::from)
    }

    /// Turn off all PSUs in the shelf.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the shelf omits its Actions
    /// container, or a BMC error if the action is not advertised or
    /// invocation fails.
    pub async fn turn_off_psus(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        self.actions()?
            .turn_off_psus(self.bmc.as_ref())
            .await
            .map_err(Error::from)
    }

    /// Type of the power shelf, for example `1RU_HPR_Power_Shelf`.
    #[must_use]
    pub fn shelf_type(&self) -> Option<&str> {
        self.data.shelf_type.as_ref().and_then(Option::as_deref)
    }

    /// Whether the shelf outputs low voltage (48V).
    #[must_use]
    pub fn voltage_output_select_low(&self) -> Option<bool> {
        self.data.voltage_output_select_low.and_then(identity)
    }

    /// Get the raw schema data for this Delta OEM power shelf.
    #[must_use]
    pub fn raw(&self) -> Arc<DeltaPowerShelfSchema> {
        self.data.clone()
    }

    fn actions(&self) -> Result<&DeltaPowerShelfActions, Error<B>> {
        self.data
            .actions
            .as_ref()
            .and_then(Option::as_ref)
            .ok_or(Error::ActionNotAvailable)
    }
}
