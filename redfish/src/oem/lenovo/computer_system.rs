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

use crate::core::Bmc;
use crate::oem::lenovo::schema::lenovo_computer_system::LenovoSystemProperties as LenovoSystemPropertiesSchema;
use crate::oem::oem_object;
use crate::schema::computer_system::ComputerSystem as ComputerSystemSchema;
use crate::Error;
use crate::NvBmc;
use std::convert::identity;
use std::marker::PhantomData;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::FpMode;
#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_computer_system::PortSwitchingTo;

/// Dell OEM Attributes.
pub struct LenovoComputerSystem<B: Bmc> {
    data: Arc<LenovoSystemPropertiesSchema>,
    _marker: PhantomData<B>,
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
        _bmc: &NvBmc<B>,
        computer_system: &ComputerSystemSchema,
    ) -> Result<Option<Self>, Error<B>> {
        Ok(computer_system
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Lenovo"))?
            .map(|data| Self {
                data,
                _marker: PhantomData,
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

    /// Front panel mode.
    pub fn front_panel_mode(&self) -> Option<FpMode> {
        self.data
            .usb_management_port_assignment
            .as_ref()
            .or_else(|| self.data.front_panel_usb.as_ref())
            .and_then(Option::as_ref)
            .and_then(|v| v.fp_mode)
            .and_then(identity)
    }

    /// USB management port switching direction.
    pub fn port_switching_to(&self) -> Option<PortSwitchingTo> {
        self.data
            .usb_management_port_assignment
            .as_ref()
            .or_else(|| self.data.front_panel_usb.as_ref())
            .and_then(Option::as_ref)
            .and_then(|v| v.port_switching_to)
            .and_then(identity)
    }
}
