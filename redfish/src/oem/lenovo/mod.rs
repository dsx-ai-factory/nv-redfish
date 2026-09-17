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

//! Support of Lenovo OEM extensions to Redfish.

#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
use crate::schema::resource::OemUpdate;
#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
use serde::Serialize;
#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
use serde_json::Value;

mod compiled_schema;

/// Key used for Lenovo values inside a Redfish OEM object.
#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
pub(crate) const OEM_KEY: &str = "Lenovo";

/// Wrap a typed Lenovo value in the standard Redfish OEM update container.
#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
pub(crate) fn oem_update<T: Serialize>(
    existing: Option<OemUpdate>,
    update: &T,
) -> Result<OemUpdate, serde_json::Error> {
    let mut additional_properties = existing
        .and_then(|oem| oem.additional_properties.as_object().cloned())
        .unwrap_or_default();
    additional_properties.insert(OEM_KEY.to_string(), serde_json::to_value(update)?);
    Ok(OemUpdate {
        additional_properties: Value::Object(additional_properties),
    })
}

/// Support of Lenovo Manager OEM attributes.
#[cfg(feature = "managers")]
pub mod manager;

/// Support of Lenovo Security service.
#[cfg(feature = "managers")]
pub mod security_service;

/// Support of Lenovo Computer System service.
#[cfg(feature = "computer-systems")]
pub mod computer_system;

/// Support of Lenovo persistent boot-order resources.
#[cfg(feature = "computer-systems")]
pub mod boot_manager;

/// Support of Lenovo ComputerSystem OEM actions.
#[cfg(feature = "computer-systems")]
pub mod computer_system_actions;

/// Support of Lenovo AccountService OEM attributes.
#[cfg(feature = "accounts")]
pub mod account_service;

/// Support of Lenovo Port OEM attributes.
#[cfg(feature = "ports")]
pub mod port;

#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use computer_system_actions::LenovoComputerSystemActions;
#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use computer_system_actions::SystemResetType;

/// Lenovo OEM Schema.
pub use compiled_schema::redfish as schema;
