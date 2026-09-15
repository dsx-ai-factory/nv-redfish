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
#[cfg(feature = "oem-dell-attributes")]
pub mod attributes;

#[cfg(feature = "job-service")]
mod job_service;
#[cfg(feature = "job-service")]
mod jobs;
#[cfg(all(
    feature = "managers",
    any(feature = "job-service", feature = "oem-dell-attributes")
))]
mod manager;

mod compiled_schema;

#[cfg(feature = "job-service")]
pub use job_service::DellJobService;
#[cfg(feature = "job-service")]
pub use jobs::DellJobs;
#[cfg(all(
    feature = "managers",
    any(feature = "job-service", feature = "oem-dell-attributes")
))]
pub use manager::DellManager;

/// Dell OEM Schema.
pub use compiled_schema::redfish as schema;
