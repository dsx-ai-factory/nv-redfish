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

//! Sometimes Redfish implementations do not perfectly match the CSDL
//! specification. What a document says is repaired by `nv-redfish-quirks`
//! on every read through [`NvBmc`](crate::NvBmc), event streams included;
//! this module keeps the quirks that change how a collection is read —
//! members to leave out, `Members` sent as `null` — and the one repair
//! decided per resource.

/// Redfish collection related patches.
#[cfg(feature = "patch-collection")]
mod collection;
#[cfg(feature = "patch-collection")]
mod payload;

#[cfg(feature = "patch-collection")]
#[doc(inline)]
pub use serde_json::Value as JsonValue;

#[cfg(feature = "patch-collection")]
#[doc(inline)]
pub use collection::CollectionWithPatch;
#[cfg(feature = "patch-collection")]
#[doc(inline)]
pub use payload::Payload;

#[cfg(feature = "patch-collection")]
use std::sync::Arc;

/// Reference to a patch function. This function should transform a JSON
/// structure to a Redfish-compatible structure.
#[cfg(feature = "patch-collection")]
pub type ReadPatchFn = Arc<dyn Fn(JsonValue) -> JsonValue + Sync + Send>;

/// Reference to a filter function. This function should filters a JSON
/// structure.
#[cfg(feature = "patch-collection")]
pub type FilterFn = Arc<dyn Fn(&JsonValue) -> bool + Sync + Send>;
