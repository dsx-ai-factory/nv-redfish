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

//! Data types that flow between the runtime and [`crate::Scheduler`] nodes:
//! readiness, cost, the layered meta model ([`WorkMeta`] + projection traits
//! + wrappers), the structural [`RoutingPath`], and [`Completion`].

use core::fmt::Debug;
use core::time::Duration;
use std::time::Instant;

/// Cost units associated with a unit of work.
///
/// A plain [`u64`] newtype. Cost-aware schedulers read it through the
/// [`HasCost`] projection on whatever meta their tree carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct CostUnits(pub u64);

impl CostUnits {
    /// Zero cost.
    pub const ZERO: Self = Self(0);

    /// Construct cost units from a raw count.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Return the raw cost value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Readiness reported by [`crate::Scheduler::update_ready`].
///
/// `ready: false` means "skip me this scan". `next_update_at` hints when
/// to re-check; `next_cost` hints the cost of the projected next item
/// (used for admission and fairness).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Readiness {
    /// Whether the node currently has work that can be selected.
    pub ready: bool,
    /// Optional time at which readiness should be re-checked.
    pub next_update_at: Option<Instant>,
    /// Optional cost of the next item.
    pub next_cost: Option<CostUnits>,
}

impl Readiness {
    /// "Ready now" with optional cost hint.
    #[must_use]
    pub const fn ready(cost: Option<CostUnits>) -> Self {
        Self {
            ready: true,
            next_update_at: None,
            next_cost: cost,
        }
    }

    /// "Not ready" with optional next-update hint.
    #[must_use]
    pub const fn not_ready(next_update_at: Option<Instant>) -> Self {
        Self {
            ready: false,
            next_update_at,
            next_cost: None,
        }
    }
}

/// LIFO stack of child indices that routes a completion back to its
/// originating leaf.
///
/// Branches push their selected child index in `take_next` and pop it in
/// `on_complete`; leaves see an empty path. The runtime forwards it
/// verbatim and never inspects it. Backed by [`Vec`] today; later phases
/// may swap in a small inline buffer without changing the API.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RoutingPath {
    inner: Vec<u32>,
}

impl RoutingPath {
    /// Empty path. The starting state at a leaf.
    #[must_use]
    pub const fn empty() -> Self {
        Self { inner: Vec::new() }
    }

    /// Push the selected child index. Called by a branch in `take_next`.
    pub fn push(&mut self, child_idx: u32) {
        self.inner.push(child_idx);
    }

    /// Pop the most recent child index. Called by a branch in `on_complete`.
    #[must_use]
    pub fn pop(&mut self) -> Option<u32> {
        self.inner.pop()
    }

    /// Number of branches that have stamped the path.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.inner.len()
    }

    /// `true` once a leaf is reached (no more branches to forward through).
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Marker bound for any work-meta type.
///
/// `WorkMeta` has no methods; it is a single-name alias for
/// `Debug + Clone + Send + 'static`. The blanket impl below adopts every
/// matching type, so users pick a meta (often `()`) without writing any
/// impls. Policy data lives in *wrappers* that implement *projection
/// traits* — [`WithCost`] adds [`HasCost`], [`WithPriority`] adds
/// [`HasPriority`].
pub trait WorkMeta: Send + 'static {}

impl<T: Send + 'static> WorkMeta for T {}

/// Projection exposing a [`CostUnits`] value from a meta type.
pub trait HasCost {
    /// Cost of the item this meta describes.
    fn cost(&self) -> CostUnits;
}

/// Projection exposing a priority class from a meta type. Higher means
/// higher priority; the numbering is up to the user.
pub trait HasPriority {
    /// Priority class for the item this meta describes.
    fn priority(&self) -> u8;
}

/// Adds a [`CostUnits`] annotation on top of any meta `M`. Cost-aware
/// branches use `WithCost<C::Meta>` as their own `Meta` and supply the
/// cost in `take_next` from a per-child table.
#[derive(Debug, Clone)]
pub struct WithCost<M> {
    /// Wrapped child meta, carried through unchanged.
    pub inner: M,
    /// Cost annotation added at this layer.
    pub cost: CostUnits,
}

impl<M> WithCost<M> {
    /// Wrap a meta with a cost annotation.
    #[must_use]
    pub const fn new(inner: M, cost: CostUnits) -> Self {
        Self { inner, cost }
    }
}

impl<M> HasCost for WithCost<M> {
    fn cost(&self) -> CostUnits {
        self.cost
    }
}

impl<M: HasPriority> HasPriority for WithCost<M> {
    fn priority(&self) -> u8 {
        self.inner.priority()
    }
}

/// Adds a priority annotation on top of any meta `M`. Priority-aware
/// branches use `WithPriority<C::Meta>` as their own `Meta` and supply
/// the priority in `take_next` from a per-child table.
#[derive(Debug, Clone)]
pub struct WithPriority<M> {
    /// Wrapped child meta, carried through unchanged.
    pub inner: M,
    /// Priority annotation added at this layer.
    pub priority: u8,
}

impl<M> WithPriority<M> {
    /// Wrap a meta with a priority annotation.
    #[must_use]
    pub const fn new(inner: M, priority: u8) -> Self {
        Self { inner, priority }
    }
}

impl<M> HasPriority for WithPriority<M> {
    fn priority(&self) -> u8 {
        self.priority
    }
}

impl<M: HasCost> HasCost for WithPriority<M> {
    fn cost(&self) -> CostUnits {
        self.inner.cost()
    }
}

/// Success-or-failure outcome reported through the scheduler tree.
/// Application events and errors flow separately through
/// [`crate::RuntimeOutput::Work`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionOutcome {
    /// `Ok(_)` from the work payload.
    Succeeded,
    /// `Err(_)` from the work payload.
    Failed,
}

/// Completion delivered to [`crate::Scheduler::on_complete`], exactly once
/// per dispatched item.
///
/// Scheduler wrappers remove their own routing tags or metadata before passing
/// the completion to the child that dispatched the work.
#[derive(Debug, Clone)]
pub struct Completion<M: WorkMeta> {
    /// Time when the driver received the job's result, using the same clock as
    /// [`crate::Scheduler::update_ready`]. Wrappers must preserve this timestamp.
    pub completed_at: Instant,

    /// Success or failure.
    pub outcome: CompletionOutcome,
    /// Wall-clock latency between dispatch and completion.
    pub latency: Duration,
    /// Layered meta as observed at this point in the tree.
    pub meta: M,
    /// Routing breadcrumb copied from the dispatched item.
    pub routing: RoutingPath,
}
