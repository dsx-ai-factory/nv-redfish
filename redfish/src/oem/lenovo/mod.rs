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

mod compiled_schema;

#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
mod update;

#[cfg(any(
    feature = "accounts",
    feature = "computer-systems",
    feature = "managers"
))]
pub(crate) use update::oem_update;

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
