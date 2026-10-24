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

//! Cost-stamping decorator.
//!
//! Adapts a cost-naive child for cost-aware branches like
//! [`crate::schedulers::TokenBucket`]: every item pulled from the child
//! gets its meta wrapped in [`WithCost`] carrying this node's current
//! cost, and readiness reports the same value as `next_cost`. The cost
//! is mutable at runtime via [`FixedCost::set_cost`].

use core::marker::PhantomData;
use std::time::Instant;

use crate::scheduler::{ScheduledWork, Scheduler};
use crate::work::{Completion, CostUnits, Readiness, WithCost};

/// Stamps a fixed [`CostUnits`] onto every item pulled from `inner`.
pub struct FixedCost<T, C: Scheduler<T>> {
    inner: C,
    cost: CostUnits,
    _t: PhantomData<fn() -> T>,
}

impl<T, C: Scheduler<T>> FixedCost<T, C> {
    /// Wrap `child`, stamping `cost` on everything it produces.
    #[must_use]
    pub const fn new(cost: CostUnits, child: C) -> Self {
        Self {
            inner: child,
            cost,
            _t: PhantomData,
        }
    }

    /// Cost currently stamped on produced items.
    #[must_use]
    pub const fn cost(&self) -> CostUnits {
        self.cost
    }

    /// Update the stamped cost. Items already dispatched keep the cost
    /// they were stamped with.
    pub const fn set_cost(&mut self, cost: CostUnits) {
        self.cost = cost;
    }
}

impl<T, C> Scheduler<T> for FixedCost<T, C>
where
    T: Send + 'static,
    C: Scheduler<T>,
{
    type Meta = WithCost<C::Meta>;

    fn update_ready(&mut self, now: Instant) -> Readiness {
        let r = self.inner.update_ready(now);
        Readiness {
            next_cost: r.ready.then_some(self.cost),
            ..r
        }
    }

    fn take_next(&mut self) -> Option<ScheduledWork<T, Self::Meta>> {
        let work = self.inner.take_next()?;
        Some(ScheduledWork {
            meta: WithCost::new(work.meta, self.cost),
            routing: work.routing,
            payload: work.payload,
        })
    }

    fn on_complete(&mut self, completion: Completion<Self::Meta>) {
        self.inner.on_complete(Completion {
            completed_at: completion.completed_at,
            outcome: completion.outcome,
            latency: completion.latency,
            meta: completion.meta.inner,
            routing: completion.routing,
        });
    }

    fn register_queue_event_sink(&mut self, sink: crate::QueueEventSink) {
        self.inner.register_queue_event_sink(sink);
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::time::Duration;
    use std::time::Instant;

    use super::FixedCost;
    use crate::scheduler::Scheduler as _;
    use crate::schedulers::tests::MockLeaf;
    use crate::work::{Completion, CompletionOutcome, CostUnits, HasCost as _};

    #[test]
    fn stamps_cost_on_items_and_readiness() {
        let leaf = MockLeaf::ready_firing(7);
        let handle = leaf.handle();
        let mut fc = FixedCost::new(CostUnits::new(3), leaf);

        let r = fc.update_ready(Instant::now());
        assert!(r.ready);
        assert_eq!(r.next_cost, Some(CostUnits::new(3)));

        let work = fc.take_next().expect("leaf fires");
        assert_eq!(work.meta.cost(), CostUnits::new(3));

        // Completion unwraps back to the child's meta.
        fc.on_complete(Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        });
        assert_eq!(handle.completion_count(), 1);
    }

    #[test]
    fn set_cost_applies_to_subsequent_items() {
        let leaf = MockLeaf::ready_firing(7);
        let mut fc = FixedCost::new(CostUnits::new(1), leaf);
        fc.set_cost(CostUnits::new(9));
        let work = fc.take_next().expect("leaf fires");
        assert_eq!(work.meta.cost(), CostUnits::new(9));
    }

    #[test]
    fn not_ready_reports_no_cost() {
        let leaf = MockLeaf::not_ready(None);
        let mut fc = FixedCost::new(CostUnits::new(3), leaf);
        let r = fc.update_ready(Instant::now());
        assert!(!r.ready);
        assert_eq!(r.next_cost, None);
    }
}
