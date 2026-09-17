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

//! Lenovo network port OEM extension.

use crate::mac_address::MacAddress;
use crate::oem::lenovo::schema::lenovo_port::Port as LenovoPortSchema;
use crate::schema::port::Port as PortSchema;
use serde_json::Error as JsonError;
use std::sync::Arc;

/// Lenovo OEM Port attributes.
pub struct LenovoPort {
    data: Arc<LenovoPortSchema>,
}

impl LenovoPort {
    /// Get the Lenovo OEM view when `Oem.Lenovo` is present.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing Lenovo port OEM data fails.
    pub(crate) fn new(port: &PortSchema) -> Result<Option<Self>, JsonError> {
        port.oem
            .as_ref()
            .and_then(|oem| oem.additional_properties.get("Lenovo"))
            .map(|data| {
                serde_json::from_value(data.clone()).map(|data| Self {
                    data: Arc::new(data),
                })
            })
            .transpose()
    }

    /// Get Lenovo's physical-port MAC address.
    #[must_use]
    pub fn physical_port_mac_address(&self) -> Option<MacAddress<'_>> {
        self.data
            .physical_port_mac_address
            .as_ref()
            .and_then(Option::as_deref)
            .map(MacAddress::new)
    }

    /// Get the raw schema data for this Lenovo port.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoPortSchema> {
        self.data.clone()
    }
}
