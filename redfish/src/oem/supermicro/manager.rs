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

//! Support Supermicro Manager OEM extension.

use crate::oem::oem_object;
use crate::oem::supermicro::kcs_interface::KcsInterface;
use crate::oem::supermicro::schema::smc_manager_extensions::Manager as SupermicroManagerSchema;
use crate::oem::supermicro::sys_lockdown::SysLockdown;
use crate::schema::manager::Manager as ManagerSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use std::sync::Arc;

/// Represents a Supermicro OEM extension to Manager schema.
pub struct SupermicroManager<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SupermicroManagerSchema>,
}

impl<B: Bmc> SupermicroManager<B> {
    /// Create a new manager OEM wrapper.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.Supermicro`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing Supermicro manager OEM data fails.
    pub(crate) fn new(bmc: &NvBmc<B>, manager: &ManagerSchema) -> Result<Option<Self>, Error<B>> {
        Ok(manager
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Supermicro"))?
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            }))
    }

    /// Get the raw schema data for this Supermicro Manager.
    #[must_use]
    pub fn raw(&self) -> Arc<SupermicroManagerSchema> {
        self.data.clone()
    }

    /// Get Supermicro KCS interface resource.
    ///
    /// Returns `Ok(None)` when KCS interface link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching KCS interface data fails.
    pub async fn kcs_interface(&self) -> Result<Option<KcsInterface<B>>, Error<B>> {
        if let Some(p) = &self.data.kcs_interface {
            KcsInterface::new(&self.bmc, p).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get Supermicro system lockdown resource.
    ///
    /// Returns `Ok(None)` when system lockdown link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching system lockdown data fails.
    pub async fn sys_lockdown(&self) -> Result<Option<SysLockdown<B>>, Error<B>> {
        if let Some(p) = &self.data.sys_lockdown {
            SysLockdown::new(&self.bmc, p).await.map(Some)
        } else {
            Ok(None)
        }
    }
}
