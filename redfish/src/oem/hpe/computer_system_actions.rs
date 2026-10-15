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

//! HPE actions advertised by a ComputerSystem.

use std::sync::Arc;

use nv_redfish_core::{ActionError, Bmc, ModificationResponse};

use crate::oem::hpe::schema::hpe_computer_system_ext::{
    Actions as HpeComputerSystemActionsSchema, HpeComputerSystemExt as HpeComputerSystemSchema,
};
use crate::oem::oem_object;
use crate::schema::computer_system::ComputerSystem as ComputerSystemSchema;
use crate::{Error, NvBmc};

#[doc(inline)]
pub use crate::oem::hpe::schema::hpe_computer_system_ext::ResetType;

/// HPE actions advertised by a ComputerSystem resource.
pub struct HpeComputerSystemActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<HpeComputerSystemSchema>,
}

impl<B: Bmc> HpeComputerSystemActions<B> {
    /// Parse actions advertised under `ComputerSystem.Oem.Hpe.Actions`.
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        computer_system: &ComputerSystemSchema,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(hpe) = computer_system.oem.as_ref().map_or_else(
            || Ok(None),
            |oem| oem_object::<HpeComputerSystemSchema, B>(oem, "Hpe"),
        )?
        else {
            return Ok(None);
        };
        Ok(Some(Self {
            bmc: bmc.clone(),
            data: hpe,
        }))
    }

    /// Invoke the advertised HPE system-reset action.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the action is not advertised,
    /// or a BMC error if invocation fails.
    pub async fn system_reset(
        &self,
        reset_type: ResetType,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        if actions.system_reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }
        actions
            .system_reset(self.bmc.as_ref(), reset_type)
            .await
            .map_err(Error::from)
    }

    /// Cycle auxiliary power through the advertised HPE action.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the action is not advertised,
    /// or a BMC error if invocation fails.
    pub async fn aux_power_cycle(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        self.system_reset(ResetType::AuxCycle).await
    }

    /// Get the raw HPE ComputerSystem OEM actions schema.
    #[must_use]
    pub fn raw(&self) -> Option<&HpeComputerSystemActionsSchema> {
        self.data.actions.as_ref()
    }
}
