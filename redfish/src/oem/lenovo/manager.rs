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

//! Support Lenovo Manager OEM extension.

use crate::oem::lenovo::schema::lenovo_manager::v0_1_0::LenovoManagerProperties as LenovoManagerV0_1Schema;
use crate::oem::lenovo::schema::lenovo_manager::v1_0_0::LenovoManagerProperties as LenovoManagerV1_0Schema;
use crate::oem::lenovo::security_service::LenovoSecurityService;
use crate::oem::oem_object;
use crate::schema::manager::Manager as ManagerSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use serde::Deserialize;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_manager::KcsState;

/// Lenovo has not incompatible schemas. One contains KCSEnabled as
/// boolean, another contains KCSEnabled as string with
/// Enabled/Disabled state.
#[derive(Deserialize)]
#[serde(untagged)]
pub enum LenovoManagerSchema {
    /// KCSEnabled as boolean schema
    V0_1(LenovoManagerV0_1Schema),
    /// KCSEnabled as state schema
    V1_0(LenovoManagerV1_0Schema),
}

/// Represents a Lenovo OEM exstension to Manager schema.
///
/// Provides access to system information and sub-resources such as processors.
pub struct LenovoManager<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<LenovoManagerSchema>,
}

impl<B: Bmc> LenovoManager<B> {
    /// Create a new manager handle.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.Lenovo`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing Lenovo manager OEM data fails.
    pub(crate) fn new(bmc: &NvBmc<B>, manager: &ManagerSchema) -> Result<Option<Self>, Error<B>> {
        Ok(manager
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Lenovo"))?
            .map(|data| Self {
                data,
                bmc: bmc.clone(),
            }))
    }

    /// Get the raw schema data for this Lenovo Manager.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoManagerSchema> {
        self.data.clone()
    }

    /// Host-side IPMI access via KCS protocol.
    #[must_use]
    pub fn kcs_enabled(&self) -> Option<KcsState> {
        match self.data.as_ref() {
            LenovoManagerSchema::V0_1(data) => data.kcs_enabled.map(|v| {
                if v {
                    KcsState::Enabled
                } else {
                    KcsState::Disabled
                }
            }),
            LenovoManagerSchema::V1_0(data) => data.kcs_enabled,
        }
    }

    /// Get lenovo security for the manager.
    ///
    /// Returns `Ok(None)` when Lenovo Security service link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching Lenovo Security service data fails.
    pub async fn security(&self) -> Result<Option<LenovoSecurityService<B>>, Error<B>> {
        let security = match self.data.as_ref() {
            LenovoManagerSchema::V0_1(data) => &data.security,
            LenovoManagerSchema::V1_0(data) => &data.security,
        };
        if let Some(p) = security {
            LenovoSecurityService::new(&self.bmc, p).await.map(Some)
        } else {
            Ok(None)
        }
    }
}
