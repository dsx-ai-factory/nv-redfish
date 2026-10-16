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
//! - [`BmcQuirks`] holds an endpoint's quirks: those of every rule the
//!   evidence its service root carries ([`RootEvidence`]) matches, by
//!   vendor, product or version. A rule only adds quirks, so a rule for one
//!   product cannot take away those its vendor's rules give it.
//!   Classification is a pure function; the caller decides what it fetches.
//! - The repair table turns the document quirks into rewrites keyed by the
//!   document's type: the schema and type its `@odata.type` names, in any
//!   version.
//! - [`CompatBmc`] is a [`Bmc`](nv_redfish_core::Bmc) over another that
//!   applies the platform's repairs to every document it reads, members a
//!   device expanded inline included. The high-level wrappers in
//!   `nv-redfish` read through it, and a consumer reading generated schema
//!   types directly gets the same repairs, all but the two the wrappers
//!   make themselves (below). Writes go through as they are,
//!   so the transport's own reading of a reply still applies. Server-sent
//!   events are repaired too, each payload under the SSE id it arrived
//!   with.
//! - [`RulesBmc`] wraps a transport with repairs the caller supplies:
//!   [`UserRule`]s matched by type ([`Match::Type`]) or `@odata.id`
//!   pattern, applied to every document read through it: after the
//!   platform's repairs on a read through the layer, and before them on a
//!   document the layer repairs afterwards, such as an event stream payload
//!   or the service root. The rules are the transport's for its whole
//!   life, so new rules take a new transport.
//!
//! The crate does not depend on the generated model, only on
//! `nv-redfish-core` and JSON values: a repair is a fact about a wire
//! document, and it must not depend on any typed reading of that document.
//! Quirks that change how a resource is navigated rather than what a
//! document says — a root without links, collection members to ignore,
//! vendor account slots — stay with the wrappers in `nv-redfish`, which
//! consult the same classification. So do two document repairs: a DPU
//! collection's `Members: null`, read as empty while the wrapper reads the
//! collection ([`BmcQuirks::bug_nullable_members`]), and the Lite-On power
//! supply repair, which needs evidence the service root does not carry,
//! the manufacturer of the chassis holding the power supplies
//! ([`BmcQuirks::power_supply_location_indicator_repair`]).

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

mod compat;
mod events;
mod fixes;
mod platform;
mod rules;
mod user;

pub use compat::CompatBmc;
pub use compat::CompatError;
pub use nv_redfish_core::DecodeError;
pub use nv_redfish_core::Raw;
pub use platform::BmcQuirks;
pub use platform::RootEvidence;
pub use user::IdPattern;
pub use user::Match;
pub use user::RulesBmc;
pub use user::UserFix;
pub use user::UserRule;
