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

//! Support Supermicro System Lockdown OEM resource.

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::oem::supermicro::schema::sys_lockdown::SysLockdown as SysLockdownSchema;
pub use crate::oem::supermicro::schema::sys_lockdown::SysLockdownUpdate;
use crate::Error;
use crate::NvBmc;
use std::sync::Arc;

/// Supermicro system lockdown resource.
pub struct SysLockdown<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SysLockdownSchema>,
}

impl<B: Bmc> SysLockdown<B> {
    /// Create a Supermicro system lockdown wrapper.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching system lockdown data fails.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SysLockdownSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this Supermicro system lockdown resource.
    #[must_use]
    pub fn raw(&self) -> Arc<SysLockdownSchema> {
        self.data.clone()
    }

    /// System lockdown enabled state.
    #[must_use]
    pub fn sys_lockdown_enabled(&self) -> Option<bool> {
        self.data.sys_lockdown_enabled
    }

    /// Update this system lockdown resource.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &SysLockdownUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<SysLockdownSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Enable or disable system lockdown.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn set_enabled(&self, enabled: bool) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update(
            &SysLockdownUpdate::builder()
                .with_sys_lockdown_enabled(enabled)
                .build(),
        )
        .await
    }
}
