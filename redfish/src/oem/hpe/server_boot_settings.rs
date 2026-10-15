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

//! HPE persistent server boot settings.

use std::sync::Arc;

use nv_redfish_core::{
    Bmc, EntityTypeRef as _, ModificationResponse, NavProperty, RedfishSettings as _,
};

use crate::oem::hpe::schema::hpe_server_boot_settings::HpeServerBootSettings as HpeServerBootSettingsSchema;
use crate::{Error, NvBmc};

#[doc(inline)]
pub use crate::oem::hpe::schema::hpe_server_boot_settings::{
    BootSource, HpeServerBootSettingsUpdate,
};

/// HPE persistent boot settings resource.
pub struct HpeServerBootSettings<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<HpeServerBootSettingsSchema>,
}

impl<B: Bmc> HpeServerBootSettings<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<HpeServerBootSettingsSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Boot sources available to the firmware.
    #[must_use]
    pub fn boot_sources(&self) -> Option<&[BootSource]> {
        self.data.boot_sources.as_deref()
    }

    /// Default persistent boot-category order.
    #[must_use]
    pub fn default_boot_order(&self) -> Option<&[String]> {
        self.data.default_boot_order.as_deref()
    }

    /// Current or pending persistent boot order represented by this resource.
    #[must_use]
    pub fn persistent_boot_config_order(&self) -> Option<&[String]> {
        self.data.persistent_boot_config_order.as_deref()
    }

    /// Get the advertised writable boot-settings object.
    ///
    /// Returns `Ok(None)` when this resource does not advertise
    /// `@Redfish.Settings`.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the settings object fails.
    pub async fn settings(&self) -> Result<Option<Self>, Error<B>> {
        match self.data.settings_object() {
            Some(settings) => Self::new(&self.bmc, &settings).await.map(Some),
            None => Ok(None),
        }
    }

    /// Update this boot settings resource.
    ///
    /// Call this method on the handle returned by [`Self::settings`] when the
    /// service advertises `@Redfish.Settings`.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &HpeServerBootSettingsUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<HpeServerBootSettingsSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Get the raw HPE boot settings schema.
    #[must_use]
    pub fn raw(&self) -> Arc<HpeServerBootSettingsSchema> {
        self.data.clone()
    }
}
