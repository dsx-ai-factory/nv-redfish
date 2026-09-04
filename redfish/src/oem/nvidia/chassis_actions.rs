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

//! Support NVIDIA Chassis OEM actions.

use crate::oem::nvidia::schema::chassis::OemActions as NvidiaChassisActionsSchema;
use crate::schema::chassis::OemActions as ChassisOemActionsSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use serde::Deserialize as _;
use std::sync::Arc;

pub use crate::oem::nvidia::schema::nvidia_chassis::NvidiaChassisResetType;

/// NVIDIA actions advertised by a Chassis resource.
///
/// The handle owns the BMC connection used to invoke its actions.
pub struct NvidiaChassisActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<NvidiaChassisActionsSchema>,
}

impl<B: Bmc> NvidiaChassisActions<B> {
    pub(crate) fn new(bmc: &NvBmc<B>, actions: &ChassisOemActionsSchema) -> Result<Self, Error<B>> {
        let data = NvidiaChassisActionsSchema::deserialize(&actions.additional_properties)
            .map_err(Error::Json)?;
        Ok(Self {
            bmc: bmc.clone(),
            data: Arc::new(data),
        })
    }

    /// Reset this chassis or its DPU with an NVIDIA reset type.
    ///
    /// # Errors
    ///
    /// Returns an error if the chassis does not advertise the NVIDIA `Reset`
    /// action or if invoking the action fails.
    pub async fn reset(
        &self,
        reset_type: NvidiaChassisResetType,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        if self.data.reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        self.data
            .reset(self.bmc.as_ref(), reset_type)
            .await
            .map_err(Error::Bmc)
    }

    /// Get the raw NVIDIA Chassis OEM actions schema.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaChassisActionsSchema> {
        self.data.clone()
    }
}
