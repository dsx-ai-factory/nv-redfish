// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Platform quirks for Redfish BMCs.
//!
//! Real BMCs deviate from the Redfish schema in known ways: a Dell firmware
//! inventory dates every component `00:00:00Z`, an AMI Viking chassis omits
//! the `ChassisType` the schema requires, a BlueField DPU reports an empty
//! `UUID`. This crate is where those facts live, below the generated model
//! and independent of it:
//!
//! - [`BmcQuirks`] classifies an endpoint into a platform class from the
//!   evidence its service root carries ([`RootEvidence`]) and answers which
//!   quirks that class is known for. Classification is a pure function; the
//!   caller decides what it fetches.
//! - The repair table turns the document quirks into rewrites keyed by the
//!   document's `@odata.type` family, composed for a resource type with
//!   [`BmcQuirks::read_patch`].
//! - [`CompatBmc`] is a [`Bmc`](nv_redfish_core::Bmc) over another that
//!   applies the classified platform's repairs to every document it reads,
//!   members a device expanded inline included, so a consumer reading
//!   generated schema types directly gets the same repairs the high-level
//!   wrappers do.
//!
//! The crate depends on `nv-redfish-core` and `serde_json` only: a repair is
//! a fact about a wire document, and it must not depend on any reading of
//! that document. Quirks that change how a resource is navigated rather
//! than what a document says — a root without links, collection members to
//! ignore, vendor account slots — stay with the wrappers in `nv-redfish`,
//! which consult the same classification.

#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::suspicious,
    clippy::complexity,
    clippy::perf
)]
#![deny(
    clippy::absolute_paths,
    clippy::todo,
    clippy::unimplemented,
    clippy::tests_outside_test_module,
    clippy::panic,
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    clippy::unused_trait_names,
    clippy::print_stdout,
    clippy::print_stderr
)]
#![deny(missing_docs)]
#![allow(clippy::doc_markdown)]

use std::sync::Arc;

mod compat;
mod fixes;
mod platform;
mod raw;
mod rules;

pub use compat::CompatBmc;
pub use compat::CompatError;
pub use platform::BmcQuirks;
pub use platform::RootEvidence;
pub use raw::Raw;

/// A composed document repair: a function from a document to the same
/// document with the platform's faults mended.
pub type ReadPatchFn = Arc<dyn Fn(serde_json::Value) -> serde_json::Value + Send + Sync>;
