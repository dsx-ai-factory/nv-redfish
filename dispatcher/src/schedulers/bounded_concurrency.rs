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

//! Single-child scheduler that caps the number of concurrently in-flight
//! items pulled from `inner`.
//!
//! Pass-through for meta and routing. When saturated, [`Scheduler::take_next`]
//! returns `None` and [`Scheduler::update_ready`] reports not-ready (it
//! can only un-stick on `on_complete`, which already runs).

use core::marker::PhantomData;
use std::num::NonZeroU32;
use std::time::Instant;

use crate::scheduler::{ScheduledWork, Scheduler};
use crate::work::{Completion, Readiness};

/// Caps the number of in-flight items pulled from `inner` to `cap`.
pub struct BoundedConcurrency<T, C: Scheduler<T>> {
    inner: C,
    cap: u32,
    in_flight: u32,
    _t: PhantomData<fn() -> T>,
}

impl<T, C: Scheduler<T>> BoundedConcurrency<T, C> {
    /// Wrap `child` with an in-flight cap of `cap`.
    pub const fn new(cap: NonZeroU32, child: C) -> Self {
        Self {
            inner: child,
            cap: cap.get(),
            in_flight: 0,
            _t: PhantomData,
        }
    }

    /// Number of items currently dispatched but not yet completed.
    #[must_use]
    pub const fn in_flight(&self) -> u32 {
        self.in_flight
    }

    /// Configured concurrency cap.
    #[must_use]
    pub const fn cap(&self) -> u32 {
        self.cap
    }
}

impl<T, C> Scheduler<T> for BoundedConcurrency<T, C>
where
    T: Send + 'static,
    C: Scheduler<T>,
{
    type Meta = C::Meta;

    fn update_ready(&mut self, now: Instant) -> Readiness {
        if self.in_flight >= self.cap {
            return Readiness::not_ready(None);
        }
        self.inner.update_ready(now)
    }

    fn take_next(&mut self) -> Option<ScheduledWork<T, C::Meta>> {
        if self.in_flight >= self.cap {
            return None;
        }
        let work = self.inner.take_next()?;
        self.in_flight = self.in_flight.saturating_add(1);
        Some(work)
    }

    fn on_complete(&mut self, completion: Completion<C::Meta>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        self.inner.on_complete(completion);
    }

    fn register_queue_event_sink(&mut self, sink: crate::QueueEventSink) {
        self.inner.register_queue_event_sink(sink);
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;
    use std::num::NonZeroU32;
    use std::time::Instant;

    use super::BoundedConcurrency;
    use crate::scheduler::Scheduler as _;
    use crate::schedulers::round_robin::RoundRobin;
    use crate::schedulers::tests::{dispatch_and_complete, MockLeaf, TestPayload};
    use crate::work::{Completion, CompletionOutcome, RoutingPath};

    fn cap(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("non-zero cap")
    }

    #[test]
    fn under_cap_passes_work_through() {
        let leaf = MockLeaf::ready_firing(7);
        let handle = leaf.handle();
        let mut bc: BoundedConcurrency<TestPayload, MockLeaf<()>> =
            BoundedConcurrency::new(cap(2), leaf);

        assert!(bc.update_ready(Instant::now()).ready);
        let work = bc.take_next().expect("ready");
        assert_eq!(work.payload, 7);
        assert_eq!(bc.in_flight(), 1);

        let completion = Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        };
        bc.on_complete(completion);
        assert_eq!(bc.in_flight(), 0);
        assert_eq!(handle.completion_count(), 1);
    }

    #[test]
    fn cap_blocks_further_dispatch_until_completion() {
        let mut rr: RoundRobin<TestPayload, ()> = RoundRobin::new();
        for id in 0..3 {
            rr.add_child(MockLeaf::ready_firing(id));
        }
        let mut bc: BoundedConcurrency<TestPayload, RoundRobin<TestPayload, ()>> =
            BoundedConcurrency::new(cap(2), rr);

        assert!(bc.update_ready(Instant::now()).ready);

        let w0 = bc.take_next().expect("first");
        let w1 = bc.take_next().expect("second");
        // Third dispatch is blocked by the cap.
        assert_eq!(bc.in_flight(), 2);
        assert!(bc.take_next().is_none());
        assert!(!bc.update_ready(Instant::now()).ready);

        // Free a slot.
        bc.on_complete(Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: w0.meta,
            routing: w0.routing,
        });
        assert_eq!(bc.in_flight(), 1);
        assert!(bc.update_ready(Instant::now()).ready);

        let w2 = bc.take_next().expect("third now allowed");
        assert_eq!(bc.in_flight(), 2);

        bc.on_complete(Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: w1.meta,
            routing: w1.routing,
        });
        bc.on_complete(Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: w2.meta,
            routing: w2.routing,
        });
        assert_eq!(bc.in_flight(), 0);
    }

    #[test]
    fn passes_meta_and_routing_unmodified() {
        let leaf = MockLeaf::ready_firing(99);
        let handle = leaf.handle();
        let mut bc: BoundedConcurrency<TestPayload, MockLeaf<()>> =
            BoundedConcurrency::new(cap(1), leaf);

        let routing =
            dispatch_and_complete(&mut bc, CompletionOutcome::Failed, Duration::from_millis(3))
                .expect("ready");

        assert_eq!(routing, RoutingPath::empty());
        assert_eq!(handle.completion_count(), 1);
        assert_eq!(
            handle.last_completion_outcome(),
            Some(CompletionOutcome::Failed)
        );
    }
}
