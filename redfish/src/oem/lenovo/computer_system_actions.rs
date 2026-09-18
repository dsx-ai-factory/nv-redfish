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

//! Lenovo ComputerSystem OEM actions.

use crate::oem::lenovo::schema::computer_system::OemActions as LenovoComputerSystemActionsSchema;
use crate::schema::computer_system::OemActions as ComputerSystemOemActionsSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use serde::Deserialize as _;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::SystemResetType;

/// Lenovo actions advertised by a ComputerSystem resource.
pub struct LenovoComputerSystemActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<LenovoComputerSystemActionsSchema>,
}

impl<B: Bmc> LenovoComputerSystemActions<B> {
    /// Parse Lenovo actions from the ComputerSystem OEM actions object.
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        actions: &ComputerSystemOemActionsSchema,
    ) -> Result<Self, Error<B>> {
        let data = LenovoComputerSystemActionsSchema::deserialize(&actions.additional_properties)
            .map_err(Error::Json)?;
        Ok(Self {
            bmc: bmc.clone(),
            data: Arc::new(data),
        })
    }

    /// Perform the Lenovo system reset operation.
    ///
    /// # Errors
    ///
    /// Returns an error if the system does not advertise this action or if
    /// invoking the action fails.
    pub async fn system_reset(
        &self,
        reset_type: SystemResetType,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        if self.data.system_reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        self.data
            .system_reset(self.bmc.as_ref(), reset_type)
            .await
            .map_err(Error::Bmc)
    }

    /// Get the raw Lenovo ComputerSystem OEM actions schema.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoComputerSystemActionsSchema> {
        self.data.clone()
    }
}
