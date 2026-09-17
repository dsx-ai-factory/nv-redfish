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

//! Lenovo SecurityService OEM extension.

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
pub use crate::oem::lenovo::schema::lenovo_security_service::ConfiguratorStateUpdate;
pub use crate::oem::lenovo::schema::lenovo_security_service::FwRollbackState;
use crate::oem::lenovo::schema::lenovo_security_service::LenovoSecurityService as LenovoSecurityServiceSchema;
pub use crate::oem::lenovo::schema::lenovo_security_service::LenovoSecurityServiceUpdate;
use crate::Error;
use crate::NvBmc;
use std::convert::identity;
use std::sync::Arc;

/// Lenovo OEM security service.
pub struct LenovoSecurityService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<LenovoSecurityServiceSchema>,
}

impl<B: Bmc> LenovoSecurityService<B> {
    /// Create Lenovo OEM security service.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<LenovoSecurityServiceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw Lenovo security service schema data.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoSecurityServiceSchema> {
        self.data.clone()
    }

    /// Update this Lenovo security service.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &LenovoSecurityServiceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<LenovoSecurityServiceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Set whether firmware rollback is allowed.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn set_fw_rollback(
        &self,
        state: FwRollbackState,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        let update = LenovoSecurityServiceUpdate::builder()
            .with_configurator(
                ConfiguratorStateUpdate::builder()
                    .with_fw_rollback(state)
                    .build(),
            )
            .build();
        self.update(&update).await
    }

    /// Firmware rollback is enabled.
    pub fn fw_rollback(&self) -> Option<FwRollbackState> {
        self.data
            .configurator
            .as_ref()
            .and_then(Option::as_ref)
            .and_then(|v| v.fw_rollback)
            .and_then(identity)
    }
}
