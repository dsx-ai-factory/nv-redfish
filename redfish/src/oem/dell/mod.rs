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

//! Support of Dell OEM extensions to Redfish.

/// Support of Dell iDRAC.
#[cfg(feature = "managers")]
pub mod attributes;

#[cfg(feature = "managers")]
mod job_service;
#[cfg(feature = "managers")]
mod jobs;
#[cfg(feature = "managers")]
mod manager;

mod compiled_schema;

#[cfg(feature = "managers")]
pub use job_service::DellJobService;
#[cfg(feature = "managers")]
pub use jobs::DellJobs;
#[cfg(feature = "managers")]
pub use manager::DellManager;

/// Dell OEM Schema.
pub use compiled_schema::redfish as schema;
