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

//! Support of Supermicro OE extensions to Redfish.

mod compiled_schema;

/// Supermicro OEM Schema.
pub use compiled_schema::redfish as schema;

/// Support of Supermicro ComputerSystem OEM extension.
#[cfg(feature = "computer-systems")]
pub mod computer_system;

/// Support of Supermicro ComputerSystem OEM actions.
#[cfg(feature = "computer-systems")]
pub mod computer_system_actions;

/// Support of Supermicro fixed boot order.
#[cfg(feature = "computer-systems")]
pub mod fixed_boot_order;

/// Support of Supermicro Manager OEM extension.
#[cfg(feature = "managers")]
pub mod manager;

/// Support of Supermicro KCS Interface service.
#[cfg(feature = "managers")]
pub mod kcs_interface;

/// Support of Supermicro System Lockdown service.
#[cfg(feature = "managers")]
pub mod sys_lockdown;

#[cfg(feature = "managers")]
#[doc(inline)]
pub use kcs_interface::KcsInterface;
#[cfg(feature = "managers")]
#[doc(inline)]
pub use kcs_interface::Privilege;
#[cfg(feature = "managers")]
#[doc(inline)]
pub use manager::ResetOption;
#[cfg(feature = "managers")]
#[doc(inline)]
pub use manager::SupermicroManager;
#[cfg(feature = "managers")]
#[doc(inline)]
pub use sys_lockdown::SysLockdown;

#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use computer_system::SupermicroComputerSystem;
#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use computer_system_actions::ResetType as SupermicroSystemResetType;
#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use computer_system_actions::SupermicroComputerSystemActions;
#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use fixed_boot_order::BootMode as SupermicroBootMode;
#[cfg(feature = "computer-systems")]
#[doc(inline)]
pub use fixed_boot_order::SmcFixedBootOrder;
