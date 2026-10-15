// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Support NVIDIA ComputerSystem OEM extension.
//!
//! The resource is the NVIDIA `NvidiaComputerSystem` OEM object and is
//! returned as the compiled schema type.
//!
//! Local CSDL additions describe the BlueField `BaseMAC`, `Mode`,
//! `HostRshim` properties and the `#Mode.Set`, `#HostRshim.Set`,
//! `#SOC.ForceReset` actions omitted by the published NVIDIA schema.
//!
//! BlueField inlines a partially expanded stub under `Oem.Nvidia`, so
//! the resource is fetched from `@odata.id` on the DPU platform.
//! Properties and actions are available whenever reported by the resource.
//! BlueField-4 reports its mode on the network adapter instead.

use crate::oem::nvidia::schema::nvidia_computer_system::Actions as NvidiaComputerSystemActions;
use crate::oem::nvidia::schema::nvidia_computer_system::NvidiaComputerSystem as NvidiaComputerSystemSchema;
use crate::oem::nvidia::OEM_KEY;
use crate::oem::oem_value;
use crate::patch_support::JsonValue;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::ActionError;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use std::sync::Arc;

pub use crate::oem::nvidia::schema::nvidia_computer_system::HostRshim;
pub use crate::oem::nvidia::schema::nvidia_computer_system::Mode;

pub use crate::oem::nvidia::BaseMac;
#[doc(hidden)]
pub use crate::oem::nvidia::BaseMacTag;

/// Represents a NVIDIA extension of computer system in the BMC.
///
/// Provides access to system information and sub-resources such as processors.
pub struct NvidiaComputerSystem<B: Bmc> {
    data: Arc<NvidiaComputerSystemSchema>,
    bmc: NvBmc<B>,
}

impl<B: Bmc> NvidiaComputerSystem<B> {
    /// Create a new computer system handle.
    ///
    /// Returns `Ok(None)` when the OEM payload carries no NVIDIA object.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        oem: &ResourceOemSchema,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(nvidia) = oem_value(oem, OEM_KEY) else {
            return Ok(None);
        };
        if bmc.quirks.bug_dpu_oem_computer_system() {
            // The inlined object is only a partially expanded stub, so
            // the body at `@odata.id` is the sole reliable source.
            let Some(id) = nvidia.get("@odata.id").and_then(JsonValue::as_str) else {
                return Ok(None);
            };
            let data = bmc.get_raw(&ODataId::from(id.to_owned())).await?;
            return Ok(Some(Self {
                data: Arc::new(data),
                bmc: bmc.clone(),
            }));
        }
        let data = serde_json::from_value(nvidia.clone()).map_err(Error::Json)?;
        Ok(Some(Self {
            data: Arc::new(data),
            bmc: bmc.clone(),
        }))
    }

    /// Get the generated actions advertised by the resource.
    fn dpu_actions(&self) -> Result<&NvidiaComputerSystemActions, Error<B>> {
        self.data.actions.as_ref().ok_or(Error::ActionNotAvailable)
    }

    /// Get the raw schema data for this NVIDIA computer system.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaComputerSystemSchema> {
        self.data.clone()
    }

    /// Get base MAC address of the device.
    ///
    /// Described by the local BlueField schema additions. Returns `None`
    /// when the resource does not report a base MAC address.
    #[must_use]
    pub fn base_mac(&self) -> Option<BaseMac<&str>> {
        self.data.base_mac.as_ref()?.as_deref().map(BaseMac::new)
    }

    /// Get mode of the Bluefield device.
    ///
    /// Described by the local BlueField schema additions. Returns `None`
    /// when the resource does not report a mode. BlueField-4 reports its
    /// mode on the network adapter instead.
    #[must_use]
    pub fn mode(&self) -> Option<Mode> {
        self.data.mode.flatten()
    }

    /// Get the state of the host-side rshim interface.
    ///
    /// Described by the local BlueField schema additions. Returns `None`
    /// when the resource does not report a host-side rshim state.
    #[must_use]
    pub fn host_rshim(&self) -> Option<HostRshim> {
        self.data.host_rshim.flatten()
    }

    /// Switch the BlueField device between NIC and DPU mode.
    ///
    /// Invokes the `#Mode.Set` action advertised in the DPU's
    /// `Oem.Nvidia` body.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the DPU does not
    /// advertise `#Mode.Set` (for example BlueField-4, which sets the
    /// mode on its network adapter), or an error if invoking the action
    /// fails.
    pub async fn set_mode(&self, mode: Mode) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self.dpu_actions()?;
        if actions.mode_set.is_none() {
            return Err(Error::ActionNotAvailable);
        }
        actions
            .mode_set(self.bmc.as_ref(), mode)
            .await
            .map_err(Error::from)
    }

    /// Enable or disable the host-side rshim interface.
    ///
    /// Invokes the `#HostRshim.Set` action advertised in the DPU's
    /// `Oem.Nvidia` body.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the DPU does not
    /// advertise `#HostRshim.Set`, or an error if invoking the action
    /// fails.
    pub async fn set_host_rshim(&self, enabled: bool) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self.dpu_actions()?;
        if actions.host_rshim_set.is_none() {
            return Err(Error::ActionNotAvailable);
        }
        let host_rshim = if enabled {
            HostRshim::Enabled
        } else {
            HostRshim::Disabled
        };
        actions
            .host_rshim_set(self.bmc.as_ref(), host_rshim)
            .await
            .map_err(Error::from)
    }

    /// Force a reset of the DPU's Arm SoC.
    ///
    /// Invokes the `#SOC.ForceReset` action advertised in the DPU's
    /// `Oem.Nvidia` body.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the DPU does not
    /// advertise `#SOC.ForceReset`, or an error if invoking the action
    /// fails.
    pub async fn soc_force_reset(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self.dpu_actions()?;
        if actions.force_reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }
        actions
            .force_reset(self.bmc.as_ref())
            .await
            .map_err(Error::from)
    }
}
