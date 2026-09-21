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

//! NVIDIA UpdateService OEM actions.

use std::sync::Arc;

use crate::oem::nvidia::schema::update_service::OemActions as NvidiaUpdateServiceActionsSchema;
use crate::schema::update_service::OemActions as UpdateServiceOemActionsSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::ActionError;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use serde::Deserialize as _;

/// NVIDIA actions advertised by an UpdateService resource.
pub struct NvidiaUpdateServiceActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<NvidiaUpdateServiceActionsSchema>,
}

impl<B: Bmc> NvidiaUpdateServiceActions<B> {
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        actions: &UpdateServiceOemActionsSchema,
    ) -> Result<Self, Error<B>> {
        let data = NvidiaUpdateServiceActionsSchema::deserialize(&actions.additional_properties)
            .map_err(Error::Json)?;
        Ok(Self {
            bmc: bmc.clone(),
            data: Arc::new(data),
        })
    }

    /// Clear NVRAM for the selected firmware inventory targets.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is unavailable or invocation fails.
    pub async fn clear_nvram(
        &self,
        targets: Vec<String>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        if self.data.clear_nvram.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        self.data
            .clear_nvram(self.bmc.as_ref(), targets)
            .await
            .map_err(Error::Bmc)
    }

    /// Commit staged images for the selected firmware inventory targets.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is unavailable or invocation fails.
    pub async fn commit_image(
        &self,
        targets: Option<Vec<String>>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        if self.data.commit_image.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        self.data
            .commit_image(self.bmc.as_ref(), targets)
            .await
            .map_err(Error::Bmc)
    }

    /// Get the raw NVIDIA UpdateService OEM actions schema.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaUpdateServiceActionsSchema> {
        self.data.clone()
    }
}
