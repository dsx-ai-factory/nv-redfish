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

//! Strict priority branch.
//!
//! Children are bucketed by priority class (`u8`). Within a class a
//! [`RoundRobin`](super::RoundRobin) handles fairness. `take_next` walks
//! classes from highest to lowest priority and stops on the first class
//! that has work; `update_ready` aggregates readiness across all classes
//! (so a lower-priority class with an earlier `next_update_at` can wake
//! the runtime).

use core::convert::TryFrom as _;
use core::marker::PhantomData;
use std::collections::BTreeMap;
use std::time::Instant;

use super::round_robin::RoundRobin;
use crate::scheduler::{RuntimeChildContainer, ScheduledWork, Scheduler};
use crate::work::{Completion, Readiness, WithPriority, WorkMeta};

/// Strict priority over `u8` priority classes (higher wins).
pub struct StrictPriority<T, C: Scheduler<T>> {
    classes: BTreeMap<u8, RoundRobin<T, C::Meta>>,
    _phantom: PhantomData<fn() -> C>,
}

impl<T, C: Scheduler<T>> Default for StrictPriority<T, C> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, C: Scheduler<T>> StrictPriority<T, C> {
    /// Empty branch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            classes: BTreeMap::new(),
            _phantom: PhantomData,
        }
    }

    /// Add `child` at the given `priority` class. Returns `(priority,
    /// child_id_within_class)`.
    pub fn add_child(&mut self, child: C, priority: u8) -> (u8, u32)
    where
        T: 'static,
    {
        let rr = self.classes.entry(priority).or_default();
        let id = rr.add_child(child);
        (priority, id)
    }

    /// Number of priority classes currently populated.
    #[must_use]
    pub fn class_count(&self) -> usize {
        self.classes.len()
    }
}

impl<T, C> Scheduler<T> for StrictPriority<T, C>
where
    T: Send + 'static,
    C: Scheduler<T>,
    C::Meta: WorkMeta,
{
    type Meta = WithPriority<C::Meta>;

    fn update_ready(&mut self, now: Instant) -> Readiness {
        let mut ready = false;
        let mut next_at: Option<Instant> = None;
        for rr in self.classes.values_mut() {
            let r = rr.update_ready(now);
            ready |= r.ready;
            next_at = match (next_at, r.next_update_at) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
        Readiness {
            ready,
            next_update_at: next_at,
            next_cost: None,
        }
    }

    fn take_next(&mut self) -> Option<ScheduledWork<T, WithPriority<C::Meta>>> {
        for (&priority, rr) in self.classes.iter_mut().rev() {
            if let Some(work) = rr.take_next() {
                let mut routing = work.routing;
                routing.push(u32::from(priority));
                let meta = WithPriority::new(work.meta, priority);
                return Some(ScheduledWork {
                    meta,
                    routing,
                    payload: work.payload,
                });
            }
        }
        None
    }

    fn on_complete(&mut self, mut completion: Completion<WithPriority<C::Meta>>) {
        let Some(prio_tag) = completion.routing.pop() else {
            return;
        };
        let priority = u8::try_from(prio_tag).unwrap_or(0);
        let inner_completion = Completion {
            completed_at: completion.completed_at,
            outcome: completion.outcome,
            latency: completion.latency,
            meta: completion.meta.inner,
            routing: completion.routing,
        };
        if let Some(rr) = self.classes.get_mut(&priority) {
            rr.on_complete(inner_completion);
        }
    }

    fn register_queue_event_sink(&mut self, sink: crate::QueueEventSink) {
        for rr in self.classes.values_mut() {
            rr.register_queue_event_sink(sink.clone());
        }
    }
}

impl<T, C> RuntimeChildContainer<T> for StrictPriority<T, C>
where
    T: Send + 'static,
    C: Scheduler<T>,
    C::Meta: WorkMeta,
{
    type ChildMeta = C::Meta;
    type ChildId = (u8, u32);
    type ChildArgs = u8;

    fn attach_child<S>(&mut self, child: S, priority: Self::ChildArgs) -> Self::ChildId
    where
        S: Scheduler<T, Meta = Self::ChildMeta>,
    {
        let class = self.classes.entry(priority).or_default();
        (priority, class.add_child(child))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::time::Duration;
    use std::time::Instant;

    use super::StrictPriority;
    use crate::scheduler::Scheduler as _;
    use crate::schedulers::tests::{dispatch_and_complete, MockLeaf, TestPayload};
    use crate::work::CompletionOutcome;

    #[test]
    fn high_priority_preempts_low() {
        let high = MockLeaf::ready_firing(100);
        let low = MockLeaf::ready_firing(200);
        let h_high = high.handle();
        let h_low = low.handle();

        let mut sp: StrictPriority<TestPayload, MockLeaf<()>> = StrictPriority::new();
        sp.add_child(low, 1);
        sp.add_child(high, 5);

        for _ in 0..5 {
            dispatch_and_complete(&mut sp, CompletionOutcome::Succeeded, Duration::ZERO)
                .expect("ready");
        }

        // High consumes everything; low never fires.
        assert_eq!(h_high.completion_count(), 5);
        assert_eq!(h_low.completion_count(), 0);
    }

    #[test]
    fn lower_advances_when_higher_is_not_ready() {
        let high = MockLeaf::ready_idle();
        let low = MockLeaf::ready_firing(200);
        let h_high = high.handle();
        let h_low = low.handle();

        let mut sp: StrictPriority<TestPayload, MockLeaf<()>> = StrictPriority::new();
        sp.add_child(low, 1);
        sp.add_child(high, 5);

        for _ in 0..3 {
            dispatch_and_complete(&mut sp, CompletionOutcome::Succeeded, Duration::ZERO)
                .expect("low must be picked");
        }

        assert_eq!(h_high.completion_count(), 0);
        assert_eq!(h_low.completion_count(), 3);
    }

    #[test]
    fn next_update_at_is_min_across_classes() {
        let now = Instant::now();
        let early = MockLeaf::not_ready(Some(now + Duration::from_millis(100))); // low prio, early
        let late = MockLeaf::not_ready(Some(now + Duration::from_millis(500))); // high prio, late

        let mut sp: StrictPriority<TestPayload, MockLeaf<()>> = StrictPriority::new();
        sp.add_child(early, 1);
        sp.add_child(late, 5);

        let r = sp.update_ready(now);
        assert!(!r.ready);
        let hint = r.next_update_at.expect("at least one class set a hint");
        assert_eq!(hint, now + Duration::from_millis(100));
    }

    #[test]
    fn completion_routes_to_correct_class_and_child() {
        let high0 = MockLeaf::ready_firing(1);
        let high1 = MockLeaf::ready_idle();
        let low0 = MockLeaf::ready_idle();
        let h_high0 = high0.handle();
        let h_high1 = high1.handle();
        let h_low0 = low0.handle();

        let mut sp: StrictPriority<TestPayload, MockLeaf<()>> = StrictPriority::new();
        sp.add_child(high0, 5);
        sp.add_child(high1, 5);
        sp.add_child(low0, 1);

        dispatch_and_complete(&mut sp, CompletionOutcome::Succeeded, Duration::ZERO)
            .expect("high0 fires");

        assert_eq!(h_high0.completion_count(), 1);
        assert_eq!(h_high1.completion_count(), 0);
        assert_eq!(h_low0.completion_count(), 0);
    }
}
