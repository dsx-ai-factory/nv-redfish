// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Redfish resource

#[cfg(feature = "resource-status")]
use crate::ResourceStatusSchema;
#[cfg(feature = "resource-status")]
use std::convert::identity;

#[doc(inline)]
#[cfg(feature = "resource-status")]
pub use crate::schema::resource::Health;

#[doc(inline)]
#[cfg(feature = "resource-status")]
pub use crate::schema::resource::State;

#[doc(inline)]
#[cfg(feature = "computer-systems")]
pub use crate::schema::resource::PowerState;

#[doc(inline)]
#[cfg(any(
    feature = "chassis",
    feature = "computer-systems",
    feature = "managers"
))]
pub use crate::schema::resource::ResetType;

/// The status and health of a resource and its children.
#[cfg(feature = "resource-status")]
#[derive(Clone, Debug)]
pub struct Status {
    /// The state of the resource.
    pub state: Option<State>,
    /// The health state of this resource in the absence of its dependent resources.
    pub health: Option<Health>,
    /// The overall health state from the view of this resource.
    pub health_rollup: Option<Health>,
}

/// Represents Redfish resource that provides it's status.
#[cfg(feature = "resource-status")]
pub trait ResourceProvidesStatus {
    /// Required function. Must be implemented for Redfish resources
    /// that provides resource status.
    fn resource_status_ref(&self) -> Option<&ResourceStatusSchema>;

    /// Status of the resource if it is provided.
    fn status(&self) -> Option<Status> {
        self.resource_status_ref().map(|status| Status {
            state: status.state.and_then(identity),
            health: status.health.and_then(identity),
            health_rollup: status.health_rollup.and_then(identity),
        })
    }
}
