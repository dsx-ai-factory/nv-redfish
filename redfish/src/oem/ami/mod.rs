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

//! Support of AMI OEM extensions to Redfish.

#[cfg(feature = "managers")]
pub mod config_bmc;
#[cfg(feature = "update-service")]
pub mod update_service;

pub mod ami_service_root;

mod compiled_schema;

/// AMI OEM schema.
pub use compiled_schema::redfish as schema;

#[doc(inline)]
pub use ami_service_root::AmiServiceRoot;
