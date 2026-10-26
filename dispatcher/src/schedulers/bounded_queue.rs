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

//! Core externally-fed bounded queue scheduler.
//!
//! The queue owns payload storage, hard-capacity enforcement, admission,
//! lifecycle, events, and statistics. A [`QueueDiscipline`] owns only
//! [`QueueEntryId`] values and decides dequeue order. Built-in disciplines
//! are feature-gated separately.
//!
//! Closing stops admission but retains the scheduler in its parent while
//! queued and in-flight work drains. Remove a queue from a dynamic parent
//! only after [`QueueLifecycle::Drained`] (or the corresponding
//! `RuntimeEvent::QueueDrained` with `runtime-events`); removal
//! before then can intentionally detach pending work from scheduling.

use core::convert::TryFrom as _;
use core::sync::atomic::{AtomicUsize, Ordering};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use crate::scheduler::{ScheduledWork, Scheduler};
use crate::work::{Completion, Readiness, WorkMeta};
use crate::{QueueEvent, QueueEventSink, QueueId};

/// Stable identity assigned by a queue to one admitted entry.
///
/// This is infrastructure identity for exact eviction; application work
/// identity belongs in [`ScheduledWork::meta`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueueEntryId(pub(crate) u64);

/// Lifecycle of an externally-fed queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueLifecycle {
    /// Producers may enqueue work.
    Open,
    /// Producers are closed, but queued or in-flight work remains.
    Draining,
    /// Producers are closed and no queued or in-flight work remains.
    Drained,
}

/// Read-only view of one admitted queue entry.
pub struct QueueEntryRef<'a, T, M: WorkMeta> {
    id: QueueEntryId,
    work: &'a ScheduledWork<T, M>,
}

impl<T, M: WorkMeta> Clone for QueueEntryRef<'_, T, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, M: WorkMeta> Copy for QueueEntryRef<'_, T, M> {}

impl<'a, T, M: WorkMeta> QueueEntryRef<'a, T, M> {
    /// Queue-local entry identity used for exact eviction.
    #[must_use]
    pub const fn id(self) -> QueueEntryId {
        self.id
    }

    /// Admitted work.
    #[must_use]
    pub const fn work(self) -> &'a ScheduledWork<T, M> {
        self.work
    }
}

struct StoredWork<T, M: WorkMeta> {
    admitted_order: u64,
    work: ScheduledWork<T, M>,
}

/// Read-only queue state supplied to an [`AdmissionPolicy`].
pub struct AdmissionContext<'a, T, M: WorkMeta> {
    capacity: usize,
    entries: &'a HashMap<QueueEntryId, StoredWork<T, M>>,
}

impl<T, M: WorkMeta> AdmissionContext<'_, T, M> {
    /// Current number of queued items.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.entries.len()
    }

    /// Hard queue capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Existing queued work in unspecified order.
    ///
    /// This is a lazy iterator over canonical queue storage and performs
    /// no allocation. Use [`Self::oldest`] when admission order matters.
    #[must_use]
    pub fn entries(&self) -> impl ExactSizeIterator<Item = QueueEntryRef<'_, T, M>> {
        self.entries.iter().map(|(&id, stored)| QueueEntryRef {
            id,
            work: &stored.work,
        })
    }

    /// Oldest currently queued entry by admission order.
    #[must_use]
    pub fn oldest(&self) -> Option<QueueEntryRef<'_, T, M>> {
        self.entries
            .iter()
            .min_by_key(|(_, stored)| stored.admitted_order)
            .map(|(&id, stored)| QueueEntryRef {
                id,
                work: &stored.work,
            })
    }
}

/// Admission decision returned by an [`AdmissionPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionDecision {
    /// Admit the incoming item.
    Admit,
    /// Reject and return the incoming item.
    Reject,
    /// Remove the identified existing item, return it, and admit incoming.
    EvictAndAdmit {
        /// Stable identity selected from [`AdmissionContext::entries`].
        id: QueueEntryId,
    },
}

/// Replaceable queue admission policy.
///
/// Policies inspect immutable queue state and incoming work and express
/// mutation only through [`AdmissionDecision`]. This keeps hard-capacity
/// enforcement and payload ownership inside the queue.
pub trait AdmissionPolicy<T, M: WorkMeta>: Send + 'static {
    /// Decide what to do with `incoming`.
    fn decide(
        &mut self,
        context: AdmissionContext<'_, T, M>,
        incoming: &ScheduledWork<T, M>,
    ) -> AdmissionDecision;
}

/// Built-in tail-drop admission: admit below capacity, reject at capacity.
#[derive(Debug, Clone, Copy, Default)]
pub struct TailDrop;

impl<T, M: WorkMeta> AdmissionPolicy<T, M> for TailDrop {
    fn decide(
        &mut self,
        context: AdmissionContext<'_, T, M>,
        _incoming: &ScheduledWork<T, M>,
    ) -> AdmissionDecision {
        if context.depth() < context.capacity() {
            AdmissionDecision::Admit
        } else {
            AdmissionDecision::Reject
        }
    }
}

/// Pluggable storage order for accepted queue entries.
///
/// The bounded queue owns all payloads; a discipline stores only entry IDs.
/// Implementations must return each accepted ID at most once and make
/// [`Self::remove`] remove it from future selection. If a malformed custom
/// discipline returns stale IDs or `None` while canonical entries remain,
/// the queue falls back to its oldest entry so work cannot be stranded.
pub trait QueueDiscipline<M>: Send + 'static {
    /// Add a new entry. `meta` may be used for classification.
    fn push(&mut self, id: QueueEntryId, meta: &M);

    /// Select and remove the next entry ID.
    fn take_next(&mut self) -> Option<QueueEntryId>;

    /// Remove `id` before dispatch. Returns whether it was present.
    fn remove(&mut self, id: QueueEntryId) -> bool;
}

/// Result of [`BoundedQueueProducer::try_push`].
pub enum EnqueueOutcome<T, M: WorkMeta> {
    /// Incoming work was admitted without an eviction.
    Admitted,
    /// Incoming work was rejected and is returned.
    Rejected(ScheduledWork<T, M>),
    /// Existing work was evicted and returned; incoming work was admitted.
    Evicted {
        /// The evicted work item.
        work: ScheduledWork<T, M>,
    },
    /// The queue was closed; incoming work is returned.
    Closed(ScheduledWork<T, M>),
}

/// Consistent snapshot of bounded queue statistics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoundedQueueStats {
    /// Runtime-assigned identity, or `None` before attachment.
    pub queue_id: Option<QueueId>,
    /// Current queued item count.
    pub depth: usize,
    /// Dispatched items whose completion has not returned.
    pub in_flight: usize,
    /// Hard queue capacity.
    pub capacity: usize,
    /// Policy or hard-capacity rejections; closed attempts are excluded.
    pub rejections: u64,
    /// Existing items evicted by admission policy.
    pub evictions: u64,
    /// Current queue lifecycle.
    pub lifecycle: QueueLifecycle,
}

struct QueueState<T, M: WorkMeta, P, D> {
    entries: HashMap<QueueEntryId, StoredWork<T, M>>,
    policy: P,
    discipline: D,
    lifecycle: QueueLifecycle,
    in_flight: usize,
    rejections: u64,
    evictions: u64,
    next_id: u64,
    next_order: u64,
    event_sink: Option<QueueEventSink>,
    queue_id: Option<QueueId>,
    drained_event_sent: bool,
}

struct QueueShared<T, M: WorkMeta, P, D> {
    capacity: usize,
    state: Mutex<QueueState<T, M, P, D>>,
    producer_count: AtomicUsize,
}

/// Scheduler leaf backed by an externally-fed bounded queue.
pub struct BoundedQueue<T, M: WorkMeta, P, D> {
    shared: Arc<QueueShared<T, M, P, D>>,
}

/// Cloneable producer handle for a [`BoundedQueue`].
pub struct BoundedQueueProducer<T, M: WorkMeta, P, D> {
    shared: Arc<QueueShared<T, M, P, D>>,
}

/// Scheduler leaf and first producer returned by [`BoundedQueueBuilder::build`].
pub type BoundedQueuePair<T, M, P, D> =
    (BoundedQueue<T, M, P, D>, BoundedQueueProducer<T, M, P, D>);

/// Builder for an externally-fed bounded queue.
///
/// Admission defaults to [`TailDrop`]. A discipline is intentionally not
/// selected by default: use `.fifo()`, or
/// [`Self::discipline`] with an SFQ or custom implementation.
pub struct BoundedQueueBuilder<P = TailDrop, D = ()> {
    capacity: NonZeroUsize,
    policy: P,
    discipline: D,
}

impl BoundedQueueBuilder {
    /// Start a queue builder with tail-drop admission.
    #[must_use]
    pub const fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            policy: TailDrop,
            discipline: (),
        }
    }
}

impl<P, D> BoundedQueueBuilder<P, D> {
    /// Replace the admission policy.
    #[must_use]
    pub fn admission_policy<P2>(self, policy: P2) -> BoundedQueueBuilder<P2, D> {
        BoundedQueueBuilder {
            capacity: self.capacity,
            policy,
            discipline: self.discipline,
        }
    }

    /// Select a queue discipline.
    #[must_use]
    pub fn discipline<D2>(self, discipline: D2) -> BoundedQueueBuilder<P, D2> {
        BoundedQueueBuilder {
            capacity: self.capacity,
            policy: self.policy,
            discipline,
        }
    }

    /// Build the scheduler leaf and its first producer.
    #[must_use]
    pub fn build<T, M>(self) -> BoundedQueuePair<T, M, P, D>
    where
        M: WorkMeta,
        P: AdmissionPolicy<T, M>,
        D: QueueDiscipline<M>,
    {
        let shared = Arc::new(QueueShared {
            capacity: self.capacity.get(),
            state: Mutex::new(QueueState {
                entries: HashMap::with_capacity(self.capacity.get()),
                policy: self.policy,
                discipline: self.discipline,
                lifecycle: QueueLifecycle::Open,
                in_flight: 0,
                rejections: 0,
                evictions: 0,
                next_id: 0,
                next_order: 0,
                event_sink: None,
                queue_id: None,
                drained_event_sent: false,
            }),
            producer_count: AtomicUsize::new(1),
        });
        (
            BoundedQueue {
                shared: shared.clone(),
            },
            BoundedQueueProducer { shared },
        )
    }
}

impl<T, M: WorkMeta, P, D> BoundedQueue<T, M, P, D> {
    /// Runtime-assigned identity, or `None` before attachment.
    #[must_use]
    pub fn queue_id(&self) -> Option<QueueId> {
        lock_state(&self.shared).queue_id
    }

    /// Snapshot queue statistics.
    #[must_use]
    pub fn stats(&self) -> BoundedQueueStats {
        snapshot(&self.shared)
    }
}

impl<T, M: WorkMeta, P, D> Clone for BoundedQueueProducer<T, M, P, D> {
    fn clone(&self) -> Self {
        self.shared.producer_count.fetch_add(1, Ordering::Relaxed);
        Self {
            shared: self.shared.clone(),
        }
    }
}

impl<T, M: WorkMeta, P, D> BoundedQueueProducer<T, M, P, D>
where
    P: AdmissionPolicy<T, M>,
    D: QueueDiscipline<M>,
{
    /// Attempt to enqueue `work` without waiting for queue space.
    ///
    /// The policy and discipline run under the queue mutex and must return
    /// promptly. Invalid admission decisions are rejected, and the hard
    /// capacity is enforced independently of policy behavior.
    ///
    /// # Panics
    ///
    /// Panics if the policy or discipline panics.
    pub fn try_push(&self, work: ScheduledWork<T, M>) -> EnqueueOutcome<T, M> {
        let (outcome, sink) = {
            let mut state = lock_state(&self.shared);
            let result = if state.lifecycle == QueueLifecycle::Open {
                let decision = {
                    let QueueState {
                        entries, policy, ..
                    } = &mut *state;
                    policy.decide(
                        AdmissionContext {
                            capacity: self.shared.capacity,
                            entries,
                        },
                        &work,
                    )
                };

                let was_empty = state.entries.is_empty();
                let outcome = state.apply_admission(decision, work, self.shared.capacity);
                let wake = was_empty
                    && !state.entries.is_empty()
                    && matches!(
                        &outcome,
                        EnqueueOutcome::Admitted | EnqueueOutcome::Evicted { .. }
                    );
                let sink = wake.then(|| state.event_sink.clone()).flatten();
                (outcome, sink)
            } else {
                (EnqueueOutcome::Closed(work), None)
            };
            drop(state);
            result
        };
        if let Some(sink) = sink {
            sink.push(QueueEvent::WakeUp);
        }
        outcome
    }

    /// Close the queue to every producer and return its resulting lifecycle.
    ///
    /// Already accepted work remains scheduled until fully drained.
    ///
    #[must_use]
    pub fn close(&self) -> QueueLifecycle {
        close_shared(&self.shared)
    }

    /// Runtime-assigned identity, or `None` before attachment.
    #[must_use]
    pub fn queue_id(&self) -> Option<QueueId> {
        lock_state(&self.shared).queue_id
    }

    /// Snapshot queue statistics.
    #[must_use]
    pub fn stats(&self) -> BoundedQueueStats {
        snapshot(&self.shared)
    }
}

impl<T, M: WorkMeta, P, D> Drop for BoundedQueueProducer<T, M, P, D> {
    fn drop(&mut self) {
        if self.shared.producer_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            let _ = close_shared(&self.shared);
        }
    }
}

fn lock_state<T, M: WorkMeta, P, D>(
    shared: &QueueShared<T, M, P, D>,
) -> MutexGuard<'_, QueueState<T, M, P, D>> {
    match shared.state.lock() {
        Ok(state) => state,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl<T, M: WorkMeta, P, D> QueueState<T, M, P, D>
where
    D: QueueDiscipline<M>,
{
    fn apply_admission(
        &mut self,
        decision: AdmissionDecision,
        work: ScheduledWork<T, M>,
        capacity: usize,
    ) -> EnqueueOutcome<T, M> {
        match decision {
            AdmissionDecision::Admit if self.entries.len() < capacity => {
                insert_work(self, work);
                EnqueueOutcome::Admitted
            }
            AdmissionDecision::EvictAndAdmit { id } => {
                if let Some(evicted) = evict_entry(self, id) {
                    self.evictions = self.evictions.saturating_add(1);
                    insert_work(self, work);
                    EnqueueOutcome::Evicted { work: evicted.work }
                } else {
                    self.rejections = self.rejections.saturating_add(1);
                    EnqueueOutcome::Rejected(work)
                }
            }
            AdmissionDecision::Admit | AdmissionDecision::Reject => {
                self.rejections = self.rejections.saturating_add(1);
                EnqueueOutcome::Rejected(work)
            }
        }
    }
}

fn insert_work<T, M: WorkMeta, P, D>(state: &mut QueueState<T, M, P, D>, work: ScheduledWork<T, M>)
where
    D: QueueDiscipline<M>,
{
    let id = next_entry_id(state);
    let admitted_order = next_admitted_order(state);
    state.discipline.push(id, &work.meta);
    state.entries.insert(
        id,
        StoredWork {
            admitted_order,
            work,
        },
    );
}

fn evict_entry<T, M: WorkMeta, P, D>(
    state: &mut QueueState<T, M, P, D>,
    id: QueueEntryId,
) -> Option<StoredWork<T, M>>
where
    D: QueueDiscipline<M>,
{
    if !state.entries.contains_key(&id) || !state.discipline.remove(id) {
        return None;
    }
    state.entries.remove(&id)
}

fn next_admitted_order<T, M: WorkMeta, P, D>(state: &mut QueueState<T, M, P, D>) -> u64 {
    if state.next_order == u64::MAX {
        rebase_admitted_order(state);
    }
    let admitted_order = state.next_order;
    state.next_order += 1;
    admitted_order
}

fn rebase_admitted_order<T, M: WorkMeta, P, D>(state: &mut QueueState<T, M, P, D>) {
    let mut ordered: Vec<_> = state
        .entries
        .iter()
        .map(|(&id, stored)| (id, stored.admitted_order))
        .collect();
    ordered.sort_unstable_by_key(|(_, admitted_order)| *admitted_order);
    for (new_order, (id, _)) in ordered.into_iter().enumerate() {
        let stored = state
            .entries
            .get_mut(&id)
            .expect("collected queue entry remains present");
        stored.admitted_order =
            u64::try_from(new_order).expect("live queue entry count fits in u64");
    }
    state.next_order =
        u64::try_from(state.entries.len()).expect("live queue entry count fits in u64");
}

fn next_entry_id<T, M: WorkMeta, P, D>(state: &mut QueueState<T, M, P, D>) -> QueueEntryId {
    loop {
        let id = QueueEntryId(state.next_id);
        state.next_id = state.next_id.wrapping_add(1);
        if !state.entries.contains_key(&id) {
            return id;
        }
    }
}

fn close_shared<T, M: WorkMeta, P, D>(shared: &QueueShared<T, M, P, D>) -> QueueLifecycle {
    let mut state = lock_state(shared);
    if state.lifecycle == QueueLifecycle::Open {
        state.lifecycle = if state.entries.is_empty() && state.in_flight == 0 {
            QueueLifecycle::Drained
        } else {
            QueueLifecycle::Draining
        };
    }
    let event = take_drained_event(&mut state);
    let lifecycle = state.lifecycle;
    drop(state);
    emit(event);
    lifecycle
}

fn snapshot<T, M: WorkMeta, P, D>(shared: &QueueShared<T, M, P, D>) -> BoundedQueueStats {
    let state = lock_state(shared);
    BoundedQueueStats {
        queue_id: state.queue_id,
        depth: state.entries.len(),
        in_flight: state.in_flight,
        capacity: shared.capacity,
        rejections: state.rejections,
        evictions: state.evictions,
        lifecycle: state.lifecycle,
    }
}

fn take_drained_event<T, M: WorkMeta, P, D>(
    state: &mut QueueState<T, M, P, D>,
) -> Option<(QueueEventSink, QueueEvent)> {
    if state.lifecycle == QueueLifecycle::Drained && !state.drained_event_sent {
        if let (Some(sink), Some(queue_id)) = (state.event_sink.clone(), state.queue_id) {
            state.drained_event_sent = true;
            return Some((sink, QueueEvent::Drained { queue_id }));
        }
    }
    None
}

fn emit(event: Option<(QueueEventSink, QueueEvent)>) {
    if let Some((sink, event)) = event {
        sink.push(event);
    }
}

impl<T, M, P, D> Scheduler<T> for BoundedQueue<T, M, P, D>
where
    T: Send + 'static,
    M: WorkMeta,
    P: AdmissionPolicy<T, M>,
    D: QueueDiscipline<M>,
{
    type Meta = M;

    fn update_ready(&mut self, _now: Instant) -> Readiness {
        let state = lock_state(&self.shared);
        if state.entries.is_empty() {
            Readiness::not_ready(None)
        } else {
            Readiness::ready(None)
        }
    }

    fn take_next(&mut self) -> Option<ScheduledWork<T, M>> {
        let mut state = lock_state(&self.shared);
        let attempts = state.entries.len().saturating_add(1);
        for _ in 0..attempts {
            let Some(id) = state.discipline.take_next() else {
                break;
            };
            if let Some(work) = take_entry(&mut state, id) {
                drop(state);
                return Some(work);
            }
        }

        let fallback = state
            .entries
            .iter()
            .min_by_key(|(_, stored)| stored.admitted_order)
            .map(|(&id, _)| id)?;
        let _ = state.discipline.remove(fallback);
        let work = take_entry(&mut state, fallback);
        drop(state);
        work
    }

    fn on_complete(&mut self, _completion: Completion<M>) {
        let mut state = lock_state(&self.shared);
        state.in_flight = state.in_flight.saturating_sub(1);
        if state.lifecycle == QueueLifecycle::Draining
            && state.entries.is_empty()
            && state.in_flight == 0
        {
            state.lifecycle = QueueLifecycle::Drained;
        }
        let event = take_drained_event(&mut state);
        drop(state);
        emit(event);
    }

    fn register_queue_event_sink(&mut self, sink: QueueEventSink) {
        let mut state = lock_state(&self.shared);
        let new_runtime = state
            .event_sink
            .as_ref()
            .is_none_or(|current| !current.belongs_to_same_runtime(&sink));
        if new_runtime {
            state.queue_id = Some(sink.allocate_queue_id());
            state.drained_event_sent = false;
        }
        state.event_sink = Some(sink);
        let event = take_drained_event(&mut state);
        drop(state);
        emit(event);
    }
}

fn take_entry<T, M: WorkMeta, P, D>(
    state: &mut QueueState<T, M, P, D>,
    id: QueueEntryId,
) -> Option<ScheduledWork<T, M>> {
    let stored = state.entries.remove(&id)?;
    state.in_flight = state.in_flight.saturating_add(1);
    Some(stored.work)
}

#[cfg(test)]
mod tests {
    #[cfg(feature = "runtime-events")]
    use core::sync::atomic::{AtomicBool, Ordering};
    #[cfg(feature = "runtime-events")]
    use core::task::Poll;
    use core::time::Duration;
    #[cfg(feature = "runtime-events")]
    use futures_util::future::poll_fn;
    #[cfg(feature = "runtime-events")]
    use futures_util::task::AtomicWaker;
    use std::num::{NonZeroU32, NonZeroUsize};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    #[cfg(feature = "runtime-events")]
    use std::sync::Arc;
    use std::thread;
    use std::time::Instant;
    use tokio::task::yield_now;
    use tokio::time::timeout;

    use super::{
        AdmissionContext, AdmissionDecision, AdmissionPolicy, BoundedQueueBuilder, EnqueueOutcome,
        QueueDiscipline, QueueEntryId, QueueLifecycle,
    };
    use crate::scheduler::{ScheduledWork, Scheduler as _};
    use crate::schedulers::{
        BoundedConcurrency, BoundedQueue, Fifo, RoundRobin, StrictPriority, TailDrop,
    };
    use crate::work::{Completion, CompletionOutcome, RoutingPath, WithPriority};
    use crate::{Runtime, RuntimeConfig, RuntimeOutput};

    const fn capacity(value: usize) -> NonZeroUsize {
        NonZeroUsize::new(value).expect("non-zero test capacity")
    }

    struct PrematureNone;

    impl<M> QueueDiscipline<M> for PrematureNone {
        fn push(&mut self, _id: QueueEntryId, _meta: &M) {}

        fn take_next(&mut self) -> Option<QueueEntryId> {
            None
        }

        fn remove(&mut self, _id: QueueEntryId) -> bool {
            false
        }
    }

    struct StaleId;

    impl<M> QueueDiscipline<M> for StaleId {
        fn push(&mut self, _id: QueueEntryId, _meta: &M) {}

        fn take_next(&mut self) -> Option<QueueEntryId> {
            Some(QueueEntryId(u64::MAX))
        }

        fn remove(&mut self, _id: QueueEntryId) -> bool {
            false
        }
    }

    struct PanicOnce {
        id: Option<QueueEntryId>,
        panicked: bool,
    }

    impl<M> QueueDiscipline<M> for PanicOnce {
        fn push(&mut self, id: QueueEntryId, _meta: &M) {
            self.id = Some(id);
        }

        #[allow(clippy::panic)]
        fn take_next(&mut self) -> Option<QueueEntryId> {
            if !self.panicked {
                self.panicked = true;
                panic!("test discipline panic");
            }
            self.id.take()
        }

        fn remove(&mut self, id: QueueEntryId) -> bool {
            if self.id == Some(id) {
                self.id = None;
                true
            } else {
                false
            }
        }
    }

    #[test]
    fn premature_none_from_custom_discipline_cannot_strand_entries() {
        let (mut queue, producer) = BoundedQueueBuilder::new(capacity(2))
            .discipline(PrematureNone)
            .build();
        let _ = producer.try_push(ScheduledWork::new((), 1_u8));
        let _ = producer.try_push(ScheduledWork::new((), 2_u8));

        assert_eq!(queue.take_next().map(|work| work.payload), Some(1));
        assert_eq!(queue.take_next().map(|work| work.payload), Some(2));
        assert_eq!(producer.stats().depth, 0);
    }

    #[test]
    fn stale_ids_from_custom_discipline_cannot_strand_entries() {
        let (mut queue, producer) = BoundedQueueBuilder::new(capacity(2))
            .discipline(StaleId)
            .build();
        let _ = producer.try_push(ScheduledWork::new((), 1_u8));
        let _ = producer.try_push(ScheduledWork::new((), 2_u8));

        assert_eq!(queue.take_next().map(|work| work.payload), Some(1));
        assert_eq!(queue.take_next().map(|work| work.payload), Some(2));
        assert_eq!(producer.stats().depth, 0);
    }

    #[test]
    fn poisoned_queue_state_is_recovered_without_stranding_entries() {
        let (mut queue, producer) = BoundedQueueBuilder::new(capacity(1))
            .discipline(PanicOnce {
                id: None,
                panicked: false,
            })
            .build();
        let _ = producer.try_push(ScheduledWork::new((), 1_u8));

        let panic = catch_unwind(AssertUnwindSafe(|| queue.take_next()));
        assert!(panic.is_err());
        assert_eq!(producer.stats().depth, 1);
        assert_eq!(producer.queue_id(), None);
        assert_eq!(queue.take_next().map(|work| work.payload), Some(1));
        assert_eq!(producer.stats().depth, 0);
    }

    #[test]
    fn tail_drop_is_bounded_and_fifo() {
        let (mut queue, producer) = BoundedQueueBuilder::new(capacity(2)).fifo().build();
        assert!(matches!(
            producer.try_push(ScheduledWork::new((), 1_u8)),
            EnqueueOutcome::Admitted
        ));
        assert!(matches!(
            producer.try_push(ScheduledWork::new((), 2)),
            EnqueueOutcome::Admitted
        ));
        assert!(matches!(
            producer.try_push(ScheduledWork::new((), 3)),
            EnqueueOutcome::Rejected(work) if work.payload == 3
        ));
        assert_eq!(producer.stats().depth, 2);
        assert_eq!(producer.stats().rejections, 1);
        assert_eq!(queue.take_next().map(|work| work.payload), Some(1));
        assert_eq!(queue.take_next().map(|work| work.payload), Some(2));
    }

    struct EarlyDrop;

    impl AdmissionPolicy<u8, u8> for EarlyDrop {
        fn decide(
            &mut self,
            context: AdmissionContext<'_, u8, u8>,
            incoming: &ScheduledWork<u8, u8>,
        ) -> AdmissionDecision {
            if incoming.meta == 0 && context.depth() >= 1 {
                AdmissionDecision::Reject
            } else {
                AdmissionDecision::Admit
            }
        }
    }

    #[test]
    fn custom_policy_can_drop_early_using_metadata() {
        let (_queue, producer) = BoundedQueueBuilder::new(capacity(4))
            .admission_policy(EarlyDrop)
            .fifo()
            .build();
        let _ = producer.try_push(ScheduledWork::new(1, 10));
        assert!(matches!(
            producer.try_push(ScheduledWork::new(0, 11)),
            EnqueueOutcome::Rejected(work) if work.payload == 11
        ));
        assert_eq!(producer.stats().depth, 1);
    }

    struct AlwaysAdmit;

    impl AdmissionPolicy<usize, ()> for AlwaysAdmit {
        fn decide(
            &mut self,
            _context: AdmissionContext<'_, usize, ()>,
            _incoming: &ScheduledWork<usize, ()>,
        ) -> AdmissionDecision {
            AdmissionDecision::Admit
        }
    }

    #[test]
    fn hard_capacity_holds_for_concurrent_producers_and_bad_policy() {
        let (_queue, producer) = BoundedQueueBuilder::new(capacity(8))
            .admission_policy(AlwaysAdmit)
            .fifo()
            .build();
        thread::scope(|scope| {
            for producer_id in 0..4 {
                let producer = producer.clone();
                scope.spawn(move || {
                    for item_id in 0..32 {
                        let payload = producer_id * 32 + item_id;
                        let _ = producer.try_push(ScheduledWork::new((), payload));
                    }
                });
            }
        });
        assert_eq!(producer.stats().depth, 8);
        assert_eq!(producer.stats().rejections, 120);
    }

    struct DropOldest;

    impl AdmissionPolicy<u8, ()> for DropOldest {
        fn decide(
            &mut self,
            context: AdmissionContext<'_, u8, ()>,
            _incoming: &ScheduledWork<u8, ()>,
        ) -> AdmissionDecision {
            if context.depth() == context.capacity() {
                AdmissionDecision::EvictAndAdmit {
                    id: context
                        .oldest()
                        .expect("full queue has an oldest entry")
                        .id(),
                }
            } else {
                AdmissionDecision::Admit
            }
        }
    }

    #[test]
    fn eviction_by_stable_id_returns_existing_work() {
        let (mut queue, producer) = BoundedQueueBuilder::new(capacity(2))
            .admission_policy(DropOldest)
            .fifo()
            .build();
        let _ = producer.try_push(ScheduledWork::new((), 1));
        let _ = producer.try_push(ScheduledWork::new((), 2));
        assert!(matches!(
            producer.try_push(ScheduledWork::new((), 3)),
            EnqueueOutcome::Evicted { work } if work.payload == 1
        ));
        assert_eq!(queue.take_next().map(|work| work.payload), Some(2));
        assert_eq!(queue.take_next().map(|work| work.payload), Some(3));
    }

    #[test]
    fn admission_order_is_rebased_before_counter_rollover() {
        let (_queue, producer) = BoundedQueueBuilder::new(capacity(4)).fifo().build();
        let _ = producer.try_push(ScheduledWork::new((), 1_u8));
        let _ = producer.try_push(ScheduledWork::new((), 2_u8));
        {
            let mut state = producer
                .shared
                .state
                .lock()
                .expect("test queue mutex is not poisoned");
            for stored in state.entries.values_mut() {
                stored.admitted_order = match stored.work.payload {
                    1 => u64::MAX - 2,
                    2 => u64::MAX - 1,
                    _ => unreachable!("only two test entries exist"),
                };
            }
            state.next_order = u64::MAX;
        }

        let _ = producer.try_push(ScheduledWork::new((), 3_u8));
        let state = producer
            .shared
            .state
            .lock()
            .expect("test queue mutex is not poisoned");
        let context = AdmissionContext {
            capacity: producer.shared.capacity,
            entries: &state.entries,
        };
        let oldest = context.oldest().map(|entry| entry.work().payload);
        let mut orders: Vec<_> = state
            .entries
            .values()
            .map(|stored| stored.admitted_order)
            .collect();
        drop(state);
        assert_eq!(oldest, Some(1));
        orders.sort_unstable();
        assert_eq!(orders, vec![0, 1, 2]);
    }

    #[test]
    fn close_tracks_queued_and_in_flight_draining() {
        let (mut queue, producer) = BoundedQueueBuilder::new(capacity(2)).fifo().build();
        let _ = producer.try_push(ScheduledWork::new((), 1_u8));
        assert_eq!(producer.close(), QueueLifecycle::Draining);
        assert_eq!(producer.close(), QueueLifecycle::Draining);

        let work = queue.take_next().expect("queued work");
        assert_eq!(producer.stats().depth, 0);
        assert_eq!(producer.stats().in_flight, 1);
        queue.on_complete(Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: RoutingPath::empty(),
        });
        assert_eq!(producer.stats().lifecycle, QueueLifecycle::Drained);
        assert_eq!(producer.stats().in_flight, 0);
        assert!(matches!(
            producer.try_push(ScheduledWork::new((), 2)),
            EnqueueOutcome::Closed(work) if work.payload == 2
        ));
    }

    #[test]
    fn composes_with_round_robin_and_bounded_concurrency() {
        let (queue, producer) = BoundedQueueBuilder::new(capacity(2)).fifo().build();
        let mut round_robin: RoundRobin<u8, ()> = RoundRobin::new();
        round_robin.add_child(queue);
        let mut root = BoundedConcurrency::new(
            NonZeroU32::new(1).expect("non-zero test concurrency"),
            round_robin,
        );
        let _ = producer.try_push(ScheduledWork::new((), 7));
        assert_eq!(root.take_next().map(|work| work.payload), Some(7));
    }

    #[tokio::test]
    async fn wake_up_signal_wakes_a_waiting_runtime_without_an_output_event() {
        type Work = crate::FutureWork<u8, ()>;

        let (queue, producer) = BoundedQueueBuilder::new(capacity(1)).fifo().build();
        let mut runtime = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            queue,
        );

        let push = async move {
            yield_now().await;
            let work: Work = Box::pin(async { Ok(vec![9]) });
            let _ = producer.try_push(ScheduledWork::new((), work));
        };
        let next = timeout(Duration::from_secs(1), runtime.next());
        let ((), output) = tokio::join!(push, next);
        assert!(matches!(
            output,
            Ok(RuntimeOutput::Work {
                result: Ok(events),
                ..
            }) if events == vec![9]
        ));
    }

    #[tokio::test]
    async fn dynamically_added_queue_receives_the_runtime_sink() {
        type Work = crate::FutureWork<u8, ()>;

        let root: RoundRobin<Work, ()> = RoundRobin::new();
        let mut runtime = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            root,
        );
        let handle = runtime.handle();
        let (queue, producer) = BoundedQueueBuilder::new(capacity(1)).fifo().build();
        assert!(handle
            .with_root_mut::<RoundRobin<Work, ()>, _>(|mut root| { root.add_child(queue) })
            .is_some());

        let push = async move {
            yield_now().await;
            let work: Work = Box::pin(async { Ok(vec![11]) });
            let _ = producer.try_push(ScheduledWork::new((), work));
        };
        let next = timeout(Duration::from_secs(1), runtime.next());
        let ((), output) = tokio::join!(push, next);
        assert!(matches!(
            output,
            Ok(RuntimeOutput::Work {
                result: Ok(events),
                ..
            }) if events == vec![11]
        ));
    }

    #[tokio::test]
    async fn dynamically_added_priority_queue_receives_the_runtime_sink() {
        type Work = crate::FutureWork<u8, ()>;
        type Queue = BoundedQueue<Work, (), TailDrop, Fifo>;
        type Root = StrictPriority<Work, Queue>;

        let mut runtime: Runtime<u8, (), WithPriority<()>> = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            Root::new(),
        );
        let handle = runtime.handle();
        let (queue, producer) = BoundedQueueBuilder::new(capacity(1)).fifo().build();
        let child_id = handle
            .with_root_mut::<Root, _>(|mut root| root.add_child_with(queue, 7))
            .expect("root downcasts");
        assert_eq!(child_id.0, 7);

        let push = async move {
            yield_now().await;
            let work: Work = Box::pin(async { Ok(vec![13]) });
            let _ = producer.try_push(ScheduledWork::new((), work));
        };
        let next = timeout(Duration::from_secs(1), runtime.next());
        let ((), output) = tokio::join!(push, next);
        assert!(matches!(
            output,
            Ok(RuntimeOutput::Work {
                result: Ok(events),
                ..
            }) if events == vec![13]
        ));
    }

    #[test]
    fn runtime_assigns_distinct_queue_ids() {
        type Work = crate::FutureWork<u8, ()>;

        let (first_queue, first_producer): (_, super::BoundedQueueProducer<Work, (), _, _>) =
            BoundedQueueBuilder::new(capacity(1)).fifo().build();
        let (second_queue, second_producer): (_, super::BoundedQueueProducer<Work, (), _, _>) =
            BoundedQueueBuilder::new(capacity(1)).fifo().build();
        assert_eq!(first_producer.queue_id(), None);
        assert_eq!(second_producer.queue_id(), None);

        let mut root: RoundRobin<Work, ()> = RoundRobin::new();
        root.add_child(first_queue);
        root.add_child(second_queue);
        let _runtime: Runtime<u8, (), ()> = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            root,
        );

        let first_id = first_producer
            .queue_id()
            .expect("runtime assigns the first queue ID");
        let second_id = second_producer
            .queue_id()
            .expect("runtime assigns the second queue ID");
        assert_ne!(first_id, second_id);
    }

    #[cfg(feature = "runtime-events")]
    #[tokio::test]
    async fn runtime_emits_work_then_queue_drained_exactly_once() {
        type Work = crate::FutureWork<u8, ()>;

        let (queue, producer) = BoundedQueueBuilder::new(capacity(1)).fifo().build();
        let mut runtime = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            queue,
        );
        let queue_id = producer.queue_id().expect("runtime assigns a queue ID");
        let work: Work = Box::pin(async { Ok(vec![10]) });
        let _ = producer.try_push(ScheduledWork::new((), work));
        assert_eq!(producer.close(), QueueLifecycle::Draining);

        assert!(matches!(
            runtime.next().await,
            RuntimeOutput::Work {
                result: Ok(events),
                ..
            } if events == vec![10]
        ));
        assert!(matches!(
            runtime.next().await,
            RuntimeOutput::Runtime(crate::RuntimeEvent::QueueDrained {
                queue_id: emitted
            }) if emitted == queue_id
        ));
        assert_eq!(producer.stats().lifecycle, QueueLifecycle::Drained);
        assert!(
            timeout(Duration::from_millis(20), runtime.next())
                .await
                .is_err(),
            "QueueDrained must be emitted exactly once"
        );
    }

    #[cfg(feature = "runtime-events")]
    #[tokio::test]
    async fn dropping_final_producer_closes_in_flight_queue_and_emits_once() {
        type Work = crate::FutureWork<u8, ()>;

        let started = Arc::new(AtomicBool::new(false));
        let released = Arc::new(AtomicBool::new(false));
        let gate_waker = Arc::new(AtomicWaker::new());
        let started_for_work = started.clone();
        let released_for_work = released.clone();
        let gate_waker_for_work = gate_waker.clone();
        let work: Work = Box::pin(poll_fn(move |cx| {
            started_for_work.store(true, Ordering::Release);
            if released_for_work.load(Ordering::Acquire) {
                return Poll::Ready(Ok(vec![12]));
            }
            gate_waker_for_work.register(cx.waker());
            if released_for_work.load(Ordering::Acquire) {
                Poll::Ready(Ok(vec![12]))
            } else {
                Poll::Pending
            }
        }));

        let (queue, producer) = BoundedQueueBuilder::new(capacity(1)).fifo().build();
        let mut runtime = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            queue,
        );
        let queue_id = producer.queue_id().expect("runtime assigns a queue ID");
        let _ = producer.try_push(ScheduledWork::new((), work));

        let drive = async {
            assert!(matches!(
                runtime.next().await,
                RuntimeOutput::Work {
                    result: Ok(events),
                    ..
                } if events == vec![12]
            ));
            assert!(matches!(
                runtime.next().await,
                RuntimeOutput::Runtime(crate::RuntimeEvent::QueueDrained {
                    queue_id: emitted
                }) if emitted == queue_id
            ));
            assert!(
                timeout(Duration::from_millis(20), runtime.next())
                    .await
                    .is_err(),
                "automatic close must emit QueueDrained exactly once"
            );
        };
        let close_and_release = async move {
            while !started.load(Ordering::Acquire) {
                yield_now().await;
            }
            drop(producer);
            released.store(true, Ordering::Release);
            gate_waker.wake();
        };
        let ((), ()) = tokio::join!(drive, close_and_release);
    }

    #[cfg(feature = "runtime-events")]
    #[tokio::test]
    async fn closing_an_empty_queue_emits_drained_immediately() {
        let (queue, producer): (
            _,
            super::BoundedQueueProducer<crate::FutureWork<u8, ()>, (), _, _>,
        ) = BoundedQueueBuilder::new(capacity(1)).fifo().build();
        assert_eq!(producer.close(), QueueLifecycle::Drained);
        let mut runtime = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: capacity(1),
                clock: crate::ClockConfig::Wallclock,
            },
            queue,
        );
        let queue_id = producer.queue_id().expect("runtime assigns a queue ID");

        assert!(matches!(
            runtime.next().await,
            RuntimeOutput::Runtime(crate::RuntimeEvent::QueueDrained {
                queue_id: emitted
            }) if emitted == queue_id
        ));
    }

    #[test]
    fn empty_closed_queue_is_immediately_drained() {
        let (_queue, producer): (_, super::BoundedQueueProducer<u8, (), _, _>) =
            BoundedQueueBuilder::new(capacity(1)).fifo().build();
        assert_eq!(producer.close(), QueueLifecycle::Drained);
        assert_eq!(producer.stats().lifecycle, QueueLifecycle::Drained);
        assert_eq!(producer.stats().depth, 0);
        assert_eq!(producer.stats().in_flight, 0);
    }
}
