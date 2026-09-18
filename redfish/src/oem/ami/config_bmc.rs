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

//! Support AMI Manager ConfigBMC OEM extension.

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::core::ODataId;
use crate::oem::ami::schema::ami_manager::ConfigBmc as ConfigBmcSchema;
use crate::oem::oem_value;
use crate::schema::manager::Manager as ManagerSchema;
use crate::Error;
use crate::NvBmc;
use serde_json::Value;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::ami::schema::ami_manager::ConfigBmcUpdate;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_manager::LockdownBiosSettingsChangeState;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_manager::LockdownBiosUpgradeDowngradeState;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_manager::LockoutBiosVariableWriteMode;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_manager::LockoutHostControlState;

/// Represents a AMI OEM exstension to Manager schema. BMC Config object.
pub struct ConfigBmc<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<ConfigBmcSchema>,
}

impl<B: Bmc> ConfigBmc<B> {
    /// Create a new BMC config handle.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.ConfigBMC`.
    ///
    /// # Errors
    ///
    /// Returns an error if failed to retrieve object data.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        manager: &ManagerSchema,
    ) -> Result<Option<Self>, Error<B>> {
        let oem = manager.oem.as_ref();
        if oem.and_then(|v| oem_value(v, "Ami")).is_some() {
            // AMI provides reference to ConfigBMC right in the Oem object.
            // {"Oem":{"ConfigBMC":""/redfish/v1/Managers/Self/Oem/ConfigBMC"}}
            if let Some(config_bmc_path) = oem
                .and_then(|oem| oem_value(oem, "ConfigBMC"))
                .and_then(|path| path.as_str())
            {
                let odata_id = ODataId::from(config_bmc_path.to_string());
                bmc.expand_property(&NavProperty::new_reference(odata_id))
                    .await
                    .map(|data| Self {
                        bmc: bmc.clone(),
                        data,
                    })
                    .map(Some)
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    /// Get the raw schema data for this BMC config.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<ConfigBmcSchema> {
        self.data.clone()
    }

    /// Apply AMI BMC lockdown configuration.
    ///
    /// AMI exposes this mutation as a `POST` to the ConfigBMC resource rather
    /// than a conventional Redfish `PATCH`:
    ///
    /// ```text
    /// POST <ConfigBMC @odata.id>
    /// {
    ///   "LockoutHostControl": "...",
    ///   "LockoutBiosVariableWriteMode": "...",
    ///   "LockdownBiosSettingsChange": "...",
    ///   "LockdownBiosUpgradeDowngrade": "..."
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn apply(
        &self,
        update: &ConfigBmcUpdate,
    ) -> Result<ModificationResponse<()>, Error<B>> {
        self.bmc
            .as_ref()
            .create::<_, Value>(self.data.odata_id(), update)
            .await
            .map(|response| response.map_entity(drop))
            .map_err(Error::Bmc)
    }
}
