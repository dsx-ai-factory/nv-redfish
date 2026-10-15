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

//! Support NVIDIA NetworkAdapter OEM extension.
//!
//! BlueField DPUs link a host privilege configuration from the network
//! adapter's `Oem.Nvidia` object: BlueField-4, and BlueField-3 with BMC
//! firmware 26.04 or later. BlueField-4 also reports its DPU operation
//! mode there; BlueField-3 does not. Both are declared by the NVIDIA OEM
//! CSDL.
//!
//! Adapter changes go through the standard
//! [`NetworkAdapter::update`](crate::chassis::NetworkAdapter::update),
//! with the NVIDIA part added by
//! [`NvidiaNetworkAdapterUpdateExt::with_oem_nvidia`]. BlueField-4
//! advertises a settings object for the adapter, so call it on the handle
//! returned by [`NetworkAdapter::settings`](crate::chassis::NetworkAdapter::settings).
//!
//! Local CSDL additions describe the BlueField-4 `BaseMAC` property,
//! which is exposed whenever the resource reports it.

use crate::oem::nvidia::schema::nvidia_host_privilege_config::NvidiaHostPrivilegeConfig as NvidiaHostPrivilegeConfigSchema;
use crate::oem::nvidia::schema::nvidia_network_adapter::NvidiaNetworkAdapter as NvidiaNetworkAdapterSchema;
use crate::oem::nvidia::OEM_KEY;
use crate::oem::oem_object;
use crate::schema::network_adapter::NetworkAdapter as NetworkAdapterSchema;
use crate::schema::network_adapter::NetworkAdapterUpdate;
use crate::schema::resource::OemUpdate;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::RedfishSettings as _;
use serde_json::Map;
use serde_json::Value as JsonValue;
use std::sync::Arc;

pub use crate::oem::nvidia::schema::nvidia_host_privilege_config::HostPrivilegeLevelInput;
pub use crate::oem::nvidia::schema::nvidia_host_privilege_config::NvidiaHostPrivilegeConfigUpdate;
pub use crate::oem::nvidia::schema::nvidia_host_privilege_config::PrivilegeModeType;
pub use crate::oem::nvidia::schema::nvidia_host_privilege_config::PrivilegeSettingsUpdate;
pub use crate::oem::nvidia::schema::nvidia_host_privilege_config::TristateValue;
pub use crate::oem::nvidia::schema::nvidia_network_adapter::DpuOperationMode;
pub use crate::oem::nvidia::schema::nvidia_network_adapter::NvidiaNetworkAdapterUpdate;
pub use crate::oem::nvidia::BaseMac;

/// Adds NVIDIA OEM settings to a standard NetworkAdapter update.
pub trait NvidiaNetworkAdapterUpdateExt: Sized {
    /// Merge NVIDIA adapter settings while preserving other OEM values.
    ///
    /// Repeated calls replace the existing `Nvidia` member but retain every
    /// other vendor member.
    ///
    /// # Errors
    ///
    /// Returns an error if the existing OEM payload is not an object or the
    /// NVIDIA update cannot be serialized.
    fn with_oem_nvidia(
        self,
        nvidia_update: NvidiaNetworkAdapterUpdate,
    ) -> Result<Self, serde_json::Error>;
}

impl NvidiaNetworkAdapterUpdateExt for NetworkAdapterUpdate {
    fn with_oem_nvidia(
        mut self,
        nvidia_update: NvidiaNetworkAdapterUpdate,
    ) -> Result<Self, serde_json::Error> {
        let mut members = match self.oem.take() {
            Some(oem) => {
                serde_json::from_value::<Map<String, JsonValue>>(oem.additional_properties)?
            }
            None => Map::new(),
        };
        members.insert(OEM_KEY.to_owned(), serde_json::to_value(nvidia_update)?);
        self.oem = Some(OemUpdate {
            additional_properties: JsonValue::Object(members),
        });
        Ok(self)
    }
}

/// NVIDIA OEM extension of a network adapter.
pub struct NvidiaNetworkAdapter<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<NvidiaNetworkAdapterSchema>,
}

impl<B: Bmc> NvidiaNetworkAdapter<B> {
    /// Parse the extension advertised under `NetworkAdapter.Oem.Nvidia`.
    ///
    /// Returns `Ok(None)` when the adapter carries no NVIDIA object.
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        adapter: &Arc<NetworkAdapterSchema>,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(oem) = adapter.oem.as_ref() else {
            return Ok(None);
        };
        Ok(oem_object(oem, OEM_KEY)?.map(|data| Self {
            bmc: bmc.clone(),
            data,
        }))
    }

    /// Get the raw NVIDIA network adapter extension.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaNetworkAdapterSchema> {
        self.data.clone()
    }

    /// Current DPU operation mode.
    #[must_use]
    pub fn dpu_operation_mode(&self) -> Option<DpuOperationMode> {
        self.data.dpu_operation_mode.flatten()
    }

    /// Base MAC address of the DPU.
    ///
    /// Described by local BlueField schema additions. Returns `None`
    /// when the resource does not report a base MAC address.
    #[must_use]
    pub fn base_mac(&self) -> Option<BaseMac<&str>> {
        self.data.base_mac.as_ref()?.as_deref().map(BaseMac::new)
    }

    /// Fetch the host privilege configuration linked from this adapter.
    ///
    /// Returns `Ok(None)` when the adapter does not link one.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the configuration fails.
    pub async fn host_privilege_config(
        &self,
    ) -> Result<Option<NvidiaHostPrivilegeConfig<B>>, Error<B>> {
        let Some(nav) = &self.data.host_privilege_config else {
            return Ok(None);
        };
        NvidiaHostPrivilegeConfig::new(&self.bmc, nav)
            .await
            .map(Some)
    }
}

/// NVIDIA host privilege configuration of a network adapter.
pub struct NvidiaHostPrivilegeConfig<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<NvidiaHostPrivilegeConfigSchema>,
}

impl<B: Bmc> NvidiaHostPrivilegeConfig<B> {
    /// Fetch the configuration behind `nav`.
    async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<NvidiaHostPrivilegeConfigSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw host privilege configuration.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaHostPrivilegeConfigSchema> {
        self.data.clone()
    }

    /// Whether the privileges follow a preset or custom settings.
    #[must_use]
    pub fn privilege_mode(&self) -> Option<PrivilegeModeType> {
        self.data.privilege_mode.flatten()
    }

    /// Level of access the host has to the DPU.
    #[must_use]
    pub fn host_privilege_level(&self) -> Option<HostPrivilegeLevelInput> {
        self.data
            .privilege_settings
            .as_ref()
            .and_then(|settings| settings.host_privilege_level.flatten())
    }

    /// Get the advertised host privilege settings object.
    ///
    /// Returns `Ok(None)` when this configuration does not advertise
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

    /// Update this host privilege configuration resource.
    ///
    /// Call this method on the handle returned by [`Self::settings`] when
    /// the service advertises `@Redfish.Settings`.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the configuration fails.
    pub async fn update(
        &self,
        update: &NvidiaHostPrivilegeConfigUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<NvidiaHostPrivilegeConfigSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Apply the `Privileged` or `Restricted` preset to every host
    /// privilege setting.
    ///
    /// Sends only `PrivilegeMode`, which the schema requires when the mode
    /// is written. Unlike setting `HostPrivilegeLevel` alone, this also
    /// moves the individual permissions the BMC requires to change
    /// together with the level. `Custom` is read-only and cannot be
    /// written; use [`Self::update`] with the generated builders for
    /// individual settings.
    ///
    /// Call this method on the handle returned by [`Self::settings`] when
    /// the service advertises `@Redfish.Settings`; the change applies after
    /// the next power cycle.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the configuration fails.
    pub async fn set_privilege_mode(
        &self,
        mode: PrivilegeModeType,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update(
            &NvidiaHostPrivilegeConfigUpdate::builder()
                .with_privilege_mode(mode)
                .build(),
        )
        .await
    }
}
