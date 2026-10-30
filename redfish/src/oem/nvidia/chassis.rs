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

//! Support NVIDIA Chassis OEM extension.
//!
//! Chassis changes go through the standard
//! [`Chassis::update`](crate::chassis::Chassis::update), with the NVIDIA
//! part added by [`NvidiaChassisUpdateExt::with_oem_nvidia`]:
//!
//! ```text
//! PATCH /redfish/v1/Chassis/{id}
//! {"Oem": {"Nvidia": {"HostManagementNetworkAccess": false}}}
//! ```

use crate::oem::nvidia::schema::nvidia_chassis::v1_14_0::NvidiaChassis as NvidiaChassisSchema;
use crate::oem::nvidia::OEM_KEY;
use crate::oem::oem_object;
use crate::schema::chassis::ChassisUpdate;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::schema::resource::OemUpdate;
use crate::Error;
use nv_redfish_core::Bmc;
use serde_json::Map;
use serde_json::Value as JsonValue;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::nvidia::schema::nvidia_chassis::v1_14_0::NvidiaChassisUpdate;

/// Adds NVIDIA OEM settings to a standard Chassis update.
pub trait NvidiaChassisUpdateExt: Sized {
    /// Merge NVIDIA chassis settings while preserving other OEM values.
    ///
    /// Repeated calls replace the existing `Nvidia` member but retain every
    /// other vendor member.
    ///
    /// # Errors
    ///
    /// Returns an error if the existing OEM payload is not an object or the
    /// NVIDIA update cannot be serialized.
    fn with_oem_nvidia(self, nvidia_update: NvidiaChassisUpdate)
        -> Result<Self, serde_json::Error>;
}

impl NvidiaChassisUpdateExt for ChassisUpdate {
    fn with_oem_nvidia(
        mut self,
        nvidia_update: NvidiaChassisUpdate,
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

/// NVIDIA OEM extension of a chassis, advertised as `Chassis.Oem.Nvidia`.
pub struct NvidiaChassis {
    data: Arc<NvidiaChassisSchema>,
}

impl NvidiaChassis {
    /// Parse the extension advertised under `Chassis.Oem.Nvidia`.
    ///
    /// Returns `Ok(None)` when the chassis carries no NVIDIA object.
    pub(crate) fn new<B: Bmc>(oem: &ResourceOemSchema) -> Result<Option<Self>, Error<B>> {
        Ok(oem_object(oem, OEM_KEY)?.map(|data| Self { data }))
    }

    /// Get the raw NVIDIA chassis extension.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaChassisSchema> {
        self.data.clone()
    }

    /// Whether the host's management network access is enabled
    /// (`HostManagementNetworkAccess`).
    ///
    /// Returns `None` when the chassis does not report it.
    #[must_use]
    pub fn host_management_network_access(&self) -> Option<bool> {
        self.data.host_management_network_access.flatten()
    }
}
