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

//! Support Lenovo Computer System OEM extension.

use crate::computer_system::ComputerSystem;
use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::oem::lenovo::boot_manager::LenovoBootManagerCollection;
use crate::oem::lenovo::oem_update;
use crate::oem::lenovo::schema::lenovo_computer_system::LenovoSystemProperties as LenovoSystemPropertiesSchema;
use crate::oem::oem_object;
use crate::patch_support::ReadPatchFn;
use crate::schema::computer_system::ComputerSystem as ComputerSystemSchema;
use crate::schema::computer_system::ComputerSystemUpdate;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use std::convert::identity;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::FpMode;
#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::LenovoSystemPropertiesUpdate;
#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::PortSwitchingTo;
#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::UsbManagementPortAssignmentUpdate;

/// Lenovo OEM ComputerSystem attributes.
pub struct LenovoComputerSystem<B: Bmc> {
    bmc: NvBmc<B>,
    computer_system_etag: Option<ODataETag>,
    computer_system_id: ODataId,
    data: Arc<LenovoSystemPropertiesSchema>,
    read_patch_fn: Option<ReadPatchFn>,
}

impl<B: Bmc> LenovoComputerSystem<B> {
    /// Create Lenovo OEM computer system.
    ///
    /// Returns `Ok(None)` when the system does not include `Oem.Lenovo`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing Lenovo computer system OEM data fails.
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        computer_system: &ComputerSystemSchema,
        read_patch_fn: Option<&ReadPatchFn>,
    ) -> Result<Option<Self>, Error<B>> {
        Ok(computer_system
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Lenovo"))?
            .map(|data| Self {
                bmc: bmc.clone(),
                computer_system_etag: computer_system.etag().cloned(),
                computer_system_id: computer_system.odata_id().clone(),
                data,
                read_patch_fn: read_patch_fn.cloned(),
            }))
    }

    /// Get the raw schema data for this Lenovo Computer system.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoSystemPropertiesSchema> {
        self.data.clone()
    }

    /// Fetch the Lenovo persistent boot-order resources.
    ///
    /// Returns `Ok(None)` when this system does not advertise the legacy
    /// Lenovo boot settings collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection cannot be fetched.
    pub async fn boot_settings(&self) -> Result<Option<LenovoBootManagerCollection<B>>, Error<B>> {
        match self.data.boot_settings.as_ref() {
            Some(settings) => LenovoBootManagerCollection::new(&self.bmc, settings)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    /// Update the Lenovo OEM properties on this computer system.
    ///
    /// # Errors
    ///
    /// Returns an error if the OEM update cannot be serialized or if updating
    /// or fetching the returned computer system fails.
    pub async fn update(
        &self,
        update: &LenovoSystemPropertiesUpdate,
    ) -> Result<ModificationResponse<ComputerSystem<B>>, Error<B>> {
        let update = ComputerSystemUpdate::builder()
            .with_oem(oem_update(None, update).map_err(Error::Json)?)
            .build();

        self.bmc
            .as_ref()
            .update::<_, NavProperty<ComputerSystemSchema>>(
                &self.computer_system_id,
                self.computer_system_etag.as_ref(),
                &update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move {
                ComputerSystem::new(&self.bmc, &nav, self.read_patch_fn.as_ref()).await
            })
            .await
    }

    /// Set front-panel USB sharing and ownership.
    ///
    /// The request uses whichever of the two observed Lenovo property names
    /// this system advertises. Returns `Ok(None)` when neither property is
    /// available.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned computer system
    /// fails.
    pub async fn set_usb_management_port(
        &self,
        mode: FpMode,
        switching_to: PortSwitchingTo,
    ) -> Result<Option<ModificationResponse<ComputerSystem<B>>>, Error<B>> {
        let assignment = UsbManagementPortAssignmentUpdate::builder()
            .with_fp_mode(mode)
            .with_port_switching_to(switching_to)
            .build();
        let update = if self
            .data
            .usb_management_port_assignment
            .as_ref()
            .and_then(Option::as_ref)
            .is_some()
        {
            LenovoSystemPropertiesUpdate::builder()
                .with_usb_management_port_assignment(assignment)
                .build()
        } else if self
            .data
            .front_panel_usb
            .as_ref()
            .and_then(Option::as_ref)
            .is_some()
        {
            LenovoSystemPropertiesUpdate::builder()
                .with_front_panel_usb(assignment)
                .build()
        } else {
            return Ok(None);
        };
        self.update(&update).await.map(Some)
    }

    /// Front panel mode.
    pub fn front_panel_mode(&self) -> Option<FpMode> {
        self.data
            .usb_management_port_assignment
            .as_ref()
            .and_then(Option::as_ref)
            .or_else(|| self.data.front_panel_usb.as_ref().and_then(Option::as_ref))
            .and_then(|v| v.fp_mode)
            .and_then(identity)
    }

    /// USB management port switching direction.
    pub fn port_switching_to(&self) -> Option<PortSwitchingTo> {
        self.data
            .usb_management_port_assignment
            .as_ref()
            .and_then(Option::as_ref)
            .or_else(|| self.data.front_panel_usb.as_ref().and_then(Option::as_ref))
            .and_then(|v| v.port_switching_to)
            .and_then(identity)
    }
}
