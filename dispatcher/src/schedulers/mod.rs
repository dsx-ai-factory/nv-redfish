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

//! Built-in composable [`crate::Scheduler`] policies.
//!
//! Each module here provides one branch primitive (or single-child
//! scheduler). All of them implement [`crate::Scheduler<T>`] and can be
//! composed with each other or with user-written leaves and branches.

mod bounded_concurrency;
mod bounded_queue;
mod circuit_breaker;
mod fixed_cost;
mod periodic_leaf;
mod queue_fifo;
mod queue_sfq;
mod round_robin;
mod strict_priority;
mod token_bucket;

pub use bounded_concurrency::BoundedConcurrency;
pub use bounded_queue::{
    AdmissionContext, AdmissionDecision, AdmissionPolicy, BoundedQueue, BoundedQueueBuilder,
    BoundedQueuePair, BoundedQueueProducer, BoundedQueueStats, EnqueueOutcome, QueueDiscipline,
    QueueEntryId, QueueEntryRef, QueueLifecycle, TailDrop,
};
pub use circuit_breaker::{BreakerState, CircuitBreaker, CircuitBreakerConfig};
pub use fixed_cost::FixedCost;
pub use periodic_leaf::PeriodicLeaf;
pub use queue_fifo::Fifo;
pub use queue_sfq::StochasticFairQueue;
pub use round_robin::{RemovedChild, RoundRobin};
pub use strict_priority::StrictPriority;
pub use token_bucket::{TokenBucket, TokenBucketConfig};

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_in_result)]

    use core::sync::atomic::{AtomicU32, Ordering};
    use core::time::Duration;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

    use crate::scheduler::{ScheduledWork, Scheduler};
    use crate::work::{Completion, CompletionOutcome, Readiness, RoutingPath, WorkMeta};

    /// Opaque payload type used by every policy unit test. The leaves just
    /// stamp a `u64` value the tests can recognise.
    pub type TestPayload = u64;

    /// Cloneable inspection handle returned by [`MockLeaf::handle`]. The
    /// scheduler-owned leaf and the test-side handle share their state
    /// through `Arc`s, so tests can poke the leaf while it lives inside a
    /// branch.
    pub struct MockLeafHandle<M: WorkMeta + Clone> {
        readiness: Arc<Mutex<Readiness>>,
        fire: Arc<Mutex<Option<TestPayload>>>,
        completions: Arc<Mutex<Vec<Completion<M>>>>,
        take_next_calls: Arc<AtomicU32>,
    }

    impl<M: WorkMeta + Clone> Clone for MockLeafHandle<M> {
        fn clone(&self) -> Self {
            Self {
                readiness: self.readiness.clone(),
                fire: self.fire.clone(),
                completions: self.completions.clone(),
                take_next_calls: self.take_next_calls.clone(),
            }
        }
    }

    impl<M: WorkMeta + Clone> MockLeafHandle<M> {
        pub fn completion_count(&self) -> usize {
            self.completions
                .lock()
                .expect("MockLeaf completions log poisoned")
                .len()
        }

        pub fn take_next_count(&self) -> u32 {
            self.take_next_calls.load(Ordering::SeqCst)
        }

        pub fn last_completion_outcome(&self) -> Option<CompletionOutcome> {
            self.completions
                .lock()
                .expect("MockLeaf completions log poisoned")
                .last()
                .map(|c| c.outcome)
        }
    }

    /// Scriptable leaf used to exercise branch policies in isolation.
    pub struct MockLeaf<M: WorkMeta + Clone> {
        meta: M,
        handle: MockLeafHandle<M>,
    }

    impl<M: WorkMeta + Clone> MockLeaf<M> {
        pub fn new(meta: M, readiness: Readiness, fire: Option<TestPayload>) -> Self {
            Self {
                meta,
                handle: MockLeafHandle {
                    readiness: Arc::new(Mutex::new(readiness)),
                    fire: Arc::new(Mutex::new(fire)),
                    completions: Arc::new(Mutex::new(Vec::new())),
                    take_next_calls: Arc::new(AtomicU32::new(0)),
                },
            }
        }

        pub fn handle(&self) -> MockLeafHandle<M> {
            self.handle.clone()
        }
    }

    impl MockLeaf<()> {
        /// Always-ready leaf that produces `payload` on every call.
        pub fn ready_firing(payload: TestPayload) -> Self {
            Self::new((), Readiness::ready(None), Some(payload))
        }

        /// Always-ready leaf with no payload to fire.
        pub fn ready_idle() -> Self {
            Self::new((), Readiness::ready(None), None)
        }

        /// Not-ready leaf with an optional `next_update_at` hint.
        pub fn not_ready(next_update_at: Option<Instant>) -> Self {
            Self::new((), Readiness::not_ready(next_update_at), None)
        }
    }

    impl<M: WorkMeta + Clone> Scheduler<TestPayload> for MockLeaf<M> {
        type Meta = M;

        fn update_ready(&mut self, _now: Instant) -> Readiness {
            *self
                .handle
                .readiness
                .lock()
                .expect("MockLeaf state poisoned")
        }

        fn take_next(&mut self) -> Option<ScheduledWork<TestPayload, M>> {
            self.handle.take_next_calls.fetch_add(1, Ordering::SeqCst);
            let payload = *self.handle.fire.lock().expect("MockLeaf state poisoned");
            let payload = payload?;
            Some(ScheduledWork::new(self.meta.clone(), payload))
        }

        fn on_complete(&mut self, completion: Completion<M>) {
            self.handle
                .completions
                .lock()
                .expect("MockLeaf completions log poisoned")
                .push(completion);
        }

        fn register_queue_event_sink(&mut self, _sink: crate::QueueEventSink) {}
    }

    /// Drive one full dispatch / completion round-trip against `sched`.
    ///
    /// Calls `sched.take_next()`; if work was produced, synthesises a
    /// [`Completion`] with the requested `outcome` / `latency` and feeds it
    /// back through `sched.on_complete`. Returns the routing path that was
    /// observed at the parent level so callers can inspect it.
    pub fn dispatch_and_complete<T, S>(
        sched: &mut S,
        outcome: CompletionOutcome,
        latency: Duration,
    ) -> Option<RoutingPath>
    where
        S: Scheduler<T>,
        S::Meta: Clone,
    {
        let work = sched.take_next()?;
        let routing_for_inspection = work.routing.clone();
        let completion = Completion {
            completed_at: Instant::now(),
            outcome,
            latency,
            meta: work.meta,
            routing: work.routing,
        };
        sched.on_complete(completion);
        Some(routing_for_inspection)
    }
}
