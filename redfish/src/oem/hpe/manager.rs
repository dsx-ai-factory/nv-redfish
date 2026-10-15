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

//! Support HPE Manager OEM extension.

use std::sync::Arc;

use nv_redfish_core::{
    ActionError, Bmc, EntityTypeRef as _, ModificationResponse, NavProperty, ODataETag, ODataId,
};

use crate::manager::Manager;
use crate::oem::hpe::date_time::HpeiLoDateTime;
use crate::oem::hpe::schema::hpei_lo::HpeiLo as HpeManagerSchema;
use crate::oem::oem_object;
use crate::schema::manager::Manager as ManagerSchema;
use crate::schema::manager::ManagerUpdate;
use crate::{Error, NvBmc};

use super::update::oem_update;

#[doc(inline)]
pub use crate::oem::hpe::schema::hpei_lo::{HpeiLoUpdate, ResetType};

/// Represents an HPE OEM extension to Manager schema.
pub struct HpeManager<B: Bmc> {
    data: Arc<HpeManagerSchema>,
    bmc: NvBmc<B>,
    manager_etag: Option<ODataETag>,
    manager_id: ODataId,
}

impl<B: Bmc> HpeManager<B> {
    /// Create a new manager OEM wrapper.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.Hpe`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing HPE manager OEM data fails.
    pub(crate) fn new(bmc: &NvBmc<B>, manager: &ManagerSchema) -> Result<Option<Self>, Error<B>> {
        Ok(manager
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Hpe"))?
            .map(|data| Self {
                data,
                bmc: bmc.clone(),
                manager_etag: manager.etag().cloned(),
                manager_id: manager.odata_id().clone(),
            }))
    }

    /// Get the raw schema data for this HPE Manager.
    #[must_use]
    pub fn raw(&self) -> Arc<HpeManagerSchema> {
        self.data.clone()
    }

    /// Host-side virtual NIC support state.
    #[must_use]
    pub fn virtual_nic_enabled(&self) -> Option<bool> {
        self.data.virtual_nic_enabled
    }

    /// Fetch the advertised iLO date and time service.
    ///
    /// Returns `Ok(None)` when `Oem.Hpe.Links.DateTimeService` is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the service fails.
    pub async fn date_time(&self) -> Result<Option<HpeiLoDateTime<B>>, Error<B>> {
        let Some(date_time) = self
            .data
            .links
            .as_ref()
            .and_then(|links| links.date_time_service.as_ref())
        else {
            return Ok(None);
        };

        HpeiLoDateTime::new(&self.bmc, date_time).await.map(Some)
    }

    /// Update the inline HPE Manager extension.
    ///
    /// # Errors
    ///
    /// Returns an error if serializing the OEM payload, updating the Manager,
    /// or fetching the returned Manager fails.
    pub async fn update(
        &self,
        update: &HpeiLoUpdate,
    ) -> Result<ModificationResponse<Manager<B>>, Error<B>> {
        let update = ManagerUpdate::builder()
            .with_oem(oem_update(None, update).map_err(Error::Json)?)
            .build();

        self.bmc
            .update::<_, NavProperty<ManagerSchema>>(
                &self.manager_id,
                self.manager_etag.as_ref(),
                &update,
            )
            .await?
            .try_map_entity_async(|nav| async move { Manager::new(&self.bmc, &nav).await })
            .await
    }

    /// Enable or disable the host-side virtual NIC.
    ///
    /// Returns `Ok(None)` when the property is not advertised.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned Manager fails.
    pub async fn set_virtual_nic_enabled(
        &self,
        enabled: bool,
    ) -> Result<Option<ModificationResponse<Manager<B>>>, Error<B>> {
        if self.data.virtual_nic_enabled.is_none() {
            return Ok(None);
        }

        self.update(
            &HpeiLoUpdate::builder()
                .with_virtual_nic_enabled(enabled)
                .build(),
        )
        .await
        .map(Some)
    }

    /// Restore iLO to factory defaults through the advertised HPE action.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the action is not advertised,
    /// or a BMC error if invocation fails.
    pub async fn reset_to_factory_defaults(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        if actions.reset_to_factory_defaults.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        actions
            .reset_to_factory_defaults(self.bmc.as_ref(), ResetType::Default)
            .await
            .map_err(Error::from)
    }
}
