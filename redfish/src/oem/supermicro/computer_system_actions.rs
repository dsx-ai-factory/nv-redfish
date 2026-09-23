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

//! Supermicro actions advertised by a ComputerSystem.

use std::sync::Arc;

use serde::Deserialize as _;

use crate::core::ActionError;
use crate::core::Bmc;
use crate::core::ModificationResponse;
use crate::oem::supermicro::schema::computer_system::OemActions as SupermicroComputerSystemActionsSchema;
pub use crate::oem::supermicro::schema::oem_system_extensions::ResetType;
use crate::schema::computer_system::OemActions as ComputerSystemOemActionsSchema;
use crate::Error;
use crate::NvBmc;

/// Supermicro actions advertised by a ComputerSystem resource.
pub struct SupermicroComputerSystemActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SupermicroComputerSystemActionsSchema>,
}

impl<B: Bmc> SupermicroComputerSystemActions<B> {
    /// Parse Supermicro actions from a standard ComputerSystem OEM actions object.
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        actions: &ComputerSystemOemActionsSchema,
    ) -> Result<Option<Self>, Error<B>> {
        let data =
            SupermicroComputerSystemActionsSchema::deserialize(&actions.additional_properties)
                .map_err(Error::Json)?;
        if data.reset.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            bmc: bmc.clone(),
            data: Arc::new(data),
        }))
    }

    /// Cycle the system's AC power when the BMC advertises the OEM action.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the action is not advertised,
    /// or a BMC error if invocation fails.
    pub async fn ac_power_cycle(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        if self.data.reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        self.data
            .reset(self.bmc.as_ref(), ResetType::AcCycle)
            .await
            .map_err(Error::Bmc)
    }

    /// Get the raw Supermicro ComputerSystem OEM actions schema.
    #[must_use]
    pub fn raw(&self) -> Arc<SupermicroComputerSystemActionsSchema> {
        self.data.clone()
    }
}
