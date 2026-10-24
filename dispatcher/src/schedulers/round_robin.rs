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

//! Plain round-robin branch with dynamic membership.
//!
//! Children live in a slab (`Vec` indexed by the `u32` id, which is also
//! the routing tag): rotation lookups are direct indexing, and a live-id
//! list keeps readiness passes at O(live) even when the slab holds freed
//! slots. Iteration order is a `VecDeque<(id, generation)>`: `take_next`
//! pops the front, asks that child, pushes the entry back, and stops on
//! the first `Some`. Children appended mid-scan are visited within one
//! cycle.
//!
//! Children can be removed at any time in amortized O(1): the queue is
//! purged lazily on pop, plus a full sweep whenever stale entries
//! outnumber the live ones, bounding the queue at 2 × live under any
//! churn pattern. A slot's generation bumps when its id is reused, so a
//! stale queue entry — surviving either a removal or a removal-then-reuse
//! — identifies itself and drops out of rotation.
//!
//! A child removed with nothing in flight is handed back
//! ([`RemovedChild::Detached`]); one with items outstanding is
//! quarantined in its slot, keeps receiving its completions, and is
//! dropped once drained ([`RemovedChild::Draining`]). The id is recycled
//! only after the drain, so id consumption is bounded by concurrent
//! children and a late completion is never misrouted to a child that
//! inherited the id.

use core::convert::TryFrom as _;
use core::marker::PhantomData;
use core::mem;
use std::collections::VecDeque;
use std::time::Instant;

use crate::scheduler::{RuntimeChildContainer, ScheduledWork, Scheduler};
use crate::work::{Completion, Readiness, WorkMeta};

enum Entry<T, M: WorkMeta> {
    /// Scheduled child.
    Live(Box<dyn Scheduler<T, Meta = M>>),
    /// Removed child still owed completions: out of rotation, forwarded
    /// completions until `in_flight` drains, then freed.
    Draining(Box<dyn Scheduler<T, Meta = M>>),
    /// Reusable slot.
    Free,
}

struct Slot<T, M: WorkMeta> {
    entry: Entry<T, M>,
    /// Bumped when the slot's id is handed out again; queue entries
    /// carry the generation they were enqueued under.
    generation: u32,
    in_flight: u32,
    /// Position in `live_ids` (valid while `entry` is `Live`).
    live_pos: usize,
}

/// Outcome of [`RoundRobin::remove_child`].
pub enum RemovedChild<T, M: WorkMeta> {
    /// Nothing was in flight: the subtree is handed back fully drained
    /// and safe to reuse.
    Detached(Box<dyn Scheduler<T, Meta = M>>),
    /// Items were still in flight: the subtree stays quarantined inside
    /// the branch, receives its remaining completions, and is dropped
    /// once drained.
    Draining,
}

/// Round robing over boxed children
pub struct RoundRobin<T, M: WorkMeta> {
    slots: Vec<Slot<T, M>>,
    /// Ids of `Live` slots, so readiness passes cost O(live) rather
    /// than O(slots); order is maintained by swap-remove.
    live_ids: Vec<u32>,
    /// Slot indices safe to hand out again.
    free: Vec<u32>,
    queue: VecDeque<(u32, u32)>,
    /// Stale rotation entries currently in `queue`; when they outnumber
    /// the live children the queue is purged, so its length stays
    /// bounded by 2 × live regardless of churn.
    stale: usize,
    _t: PhantomData<fn() -> T>,
}

impl<T, M: WorkMeta> Default for RoundRobin<T, M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, M: WorkMeta> RoundRobin<T, M> {
    /// Empty round-robin branch.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            live_ids: Vec::new(),
            free: Vec::new(),
            queue: VecDeque::new(),
            stale: 0,
            _t: PhantomData,
        }
    }

    /// Append `child` and return its id (the routing tag). Ids of
    /// removed children are recycled: a cached id is invalidated by
    /// [`Self::remove_child`] and may later address a different child —
    /// do not hold ids across a removal you don't control.
    ///
    /// # Panics
    ///
    /// Panics if more than `u32::MAX` children are held *concurrently*
    /// (live plus removed-but-draining), which the
    /// [`RoutingPath`](crate::RoutingPath) tag width does not support.
    pub fn add_child<S>(&mut self, child: S) -> u32
    where
        S: Scheduler<T, Meta = M>,
    {
        let live_pos = self.live_ids.len();
        let (id, generation) = if let Some(id) = self.free.pop() {
            let slot = self
                .slots
                .get_mut(id as usize)
                .expect("free list holds valid slot indices");
            slot.generation = slot.generation.wrapping_add(1);
            slot.entry = Entry::Live(Box::new(child));
            slot.in_flight = 0;
            slot.live_pos = live_pos;
            (id, slot.generation)
        } else {
            let id = u32::try_from(self.slots.len())
                .expect("RoundRobin supports up to u32::MAX concurrent children");
            self.slots.push(Slot {
                entry: Entry::Live(Box::new(child)),
                generation: 0,
                in_flight: 0,
                live_pos,
            });
            (id, 0)
        };
        self.live_ids.push(id);
        self.queue.push_back((id, generation));
        id
    }

    /// Remove the child with the given id, or `None` if no such child
    /// exists. Amortized O(1): the rotation queue is purged lazily.
    ///
    /// With nothing in flight the subtree is returned
    /// ([`RemovedChild::Detached`]); otherwise it stays quarantined —
    /// out of rotation, still receiving its remaining completions — and
    /// is dropped once drained ([`RemovedChild::Draining`]). The id is
    /// reusable by [`Self::add_child`] only after the drain finishes.
    pub fn remove_child(&mut self, id: u32) -> Option<RemovedChild<T, M>> {
        let slot = self.slots.get_mut(id as usize)?;
        let removed = match mem::replace(&mut slot.entry, Entry::Free) {
            Entry::Live(sched) => {
                if slot.in_flight > 0 {
                    slot.entry = Entry::Draining(sched);
                    RemovedChild::Draining
                } else {
                    self.free.push(id);
                    RemovedChild::Detached(sched)
                }
            }
            other => {
                slot.entry = other;
                return None;
            }
        };

        let pos = slot.live_pos;
        self.live_ids.swap_remove(pos);
        if let Some(&moved) = self.live_ids.get(pos) {
            if let Some(moved_slot) = self.slots.get_mut(moved as usize) {
                moved_slot.live_pos = pos;
            }
        }

        // The removed child's rotation entry is now stale; purge the
        // queue once stale entries outnumber the live ones, so churn
        // cannot grow it without bound.
        self.stale += 1;
        if self.stale > self.live_ids.len() {
            let slots = &self.slots;
            self.queue.retain(|&(qid, qgen)| {
                slots
                    .get(qid as usize)
                    .is_some_and(|s| s.generation == qgen && matches!(s.entry, Entry::Live(_)))
            });
            self.stale = 0;
        }
        Some(removed)
    }

    /// Number of children currently held by this branch.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.live_ids.len()
    }

    /// `true` when the branch currently holds no children.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.live_ids.is_empty()
    }

    #[cfg(test)]
    fn queue_len(&self) -> usize {
        self.queue.len()
    }
}

impl<T, M> Scheduler<T> for RoundRobin<T, M>
where
    T: Send + 'static,
    M: WorkMeta,
{
    type Meta = M;

    fn update_ready(&mut self, now: Instant) -> Readiness {
        let mut ready = false;
        let mut next_at: Option<Instant> = None;
        for &id in &self.live_ids {
            let Some(slot) = self.slots.get_mut(id as usize) else {
                continue;
            };
            let Entry::Live(sched) = &mut slot.entry else {
                continue;
            };
            let r = sched.update_ready(now);
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

    fn take_next(&mut self) -> Option<ScheduledWork<T, M>> {
        let n = self.queue.len();
        for _ in 0..n {
            let (id, generation) = self.queue.pop_front()?;
            // Lazy purge: entries whose slot was removed (not Live) or
            // reused (generation mismatch) drop out of rotation here.
            let Some(slot) = self.slots.get_mut(id as usize) else {
                self.stale = self.stale.saturating_sub(1);
                continue;
            };
            if slot.generation != generation {
                self.stale = self.stale.saturating_sub(1);
                continue;
            }
            let Entry::Live(sched) = &mut slot.entry else {
                self.stale = self.stale.saturating_sub(1);
                continue;
            };
            let work = sched.take_next();
            self.queue.push_back((id, generation));
            if let Some(mut work) = work {
                slot.in_flight = slot.in_flight.saturating_add(1);
                work.routing.push(id);
                return Some(work);
            }
        }
        None
    }

    fn on_complete(&mut self, mut completion: Completion<M>) {
        let Some(id) = completion.routing.pop() else {
            return;
        };
        let Some(slot) = self.slots.get_mut(id as usize) else {
            return;
        };
        let drained = match &mut slot.entry {
            // Stale completion for a freed slot: leave its state alone.
            Entry::Free => return,
            Entry::Live(sched) => {
                slot.in_flight = slot.in_flight.saturating_sub(1);
                sched.on_complete(completion);
                false
            }
            // Forward to the quarantined child; recycle the id and drop
            // the subtree once drained.
            Entry::Draining(sched) => {
                slot.in_flight = slot.in_flight.saturating_sub(1);
                sched.on_complete(completion);
                slot.in_flight == 0
            }
        };
        if drained {
            slot.entry = Entry::Free;
            self.free.push(id);
        }
    }

    fn register_queue_event_sink(&mut self, sink: crate::QueueEventSink) {
        for slot in &mut self.slots {
            match &mut slot.entry {
                Entry::Live(scheduler) | Entry::Draining(scheduler) => {
                    scheduler.register_queue_event_sink(sink.clone());
                }
                Entry::Free => {}
            }
        }
    }
}

impl<T, M> RuntimeChildContainer<T> for RoundRobin<T, M>
where
    T: Send + 'static,
    M: WorkMeta,
{
    type ChildMeta = M;
    type ChildId = u32;
    type ChildArgs = ();

    fn attach_child<S>(&mut self, child: S, (): Self::ChildArgs) -> Self::ChildId
    where
        S: Scheduler<T, Meta = M>,
    {
        self.add_child(child)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::time::Duration;
    use std::time::Instant;

    use super::{RemovedChild, RoundRobin};
    use crate::scheduler::Scheduler as _;
    use crate::schedulers::tests::{dispatch_and_complete, MockLeaf};
    use crate::work::{Completion, CompletionOutcome};

    #[test]
    fn empty_branch_is_not_ready_and_yields_nothing() {
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let now = Instant::now();
        let r = rr.update_ready(now);
        assert!(!r.ready);
        assert!(rr.take_next().is_none());
    }

    #[test]
    fn single_child_round_trip() {
        let leaf = MockLeaf::ready_firing(11);
        let handle = leaf.handle();
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let id = rr.add_child(leaf);
        assert_eq!(id, 0);

        let now = Instant::now();
        assert!(rr.update_ready(now).ready);

        let routing = dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO)
            .expect("work should be dispatched");
        // The routing path observed at the parent already contains the
        // child id stamped by RR.
        assert_eq!(routing.depth(), 1);
        assert_eq!(handle.completion_count(), 1);
    }

    #[test]
    fn three_children_rotate_evenly() {
        let l0 = MockLeaf::ready_firing(100);
        let l1 = MockLeaf::ready_firing(200);
        let l2 = MockLeaf::ready_firing(300);
        let h0 = l0.handle();
        let h1 = l1.handle();
        let h2 = l2.handle();

        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        rr.add_child(l0);
        rr.add_child(l1);
        rr.add_child(l2);

        for _ in 0..6 {
            dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO)
                .expect("ready leaf should produce work");
        }

        assert_eq!(h0.completion_count(), 2);
        assert_eq!(h1.completion_count(), 2);
        assert_eq!(h2.completion_count(), 2);
    }

    #[test]
    fn skips_not_ready_children() {
        let l0 = MockLeaf::ready_firing(1);
        let l1 = MockLeaf::ready_idle();
        let l2 = MockLeaf::ready_firing(3);
        let h0 = l0.handle();
        let h1 = l1.handle();
        let h2 = l2.handle();

        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        rr.add_child(l0);
        rr.add_child(l1);
        rr.add_child(l2);

        for _ in 0..4 {
            dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO)
                .expect("at least one leaf is firing");
        }

        // l1 never fires, the other two split the work.
        assert_eq!(h0.completion_count() + h2.completion_count(), 4);
        assert_eq!(h1.completion_count(), 0);
    }

    #[test]
    fn add_child_mid_scan_is_visited_within_one_cycle() {
        let l0 = MockLeaf::ready_firing(1);
        let l1 = MockLeaf::ready_firing(2);
        let h0 = l0.handle();
        let h1 = l1.handle();

        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        rr.add_child(l0);
        rr.add_child(l1);

        // Pull one item to advance the cursor.
        dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO).expect("ok");

        // Add a third child mid-stream.
        let l2 = MockLeaf::ready_firing(3);
        let h2 = l2.handle();
        rr.add_child(l2);

        // Pull three items: every child must be visited at least once.
        for _ in 0..3 {
            dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO)
                .expect("ok");
        }

        assert!(h0.completion_count() >= 1);
        assert!(h1.completion_count() >= 1);
        assert!(h2.completion_count() >= 1);
    }

    #[test]
    fn removed_child_is_no_longer_scheduled() {
        let l0 = MockLeaf::ready_firing(1);
        let l1 = MockLeaf::ready_firing(2);
        let h0 = l0.handle();
        let h1 = l1.handle();

        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let id0 = rr.add_child(l0);
        rr.add_child(l1);
        assert_eq!(rr.len(), 2);

        assert!(rr.remove_child(id0).is_some());
        assert!(rr.remove_child(id0).is_none(), "second removal is a miss");
        assert_eq!(rr.len(), 1);

        for _ in 0..4 {
            dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO)
                .expect("remaining leaf fires");
        }
        assert_eq!(h0.completion_count(), 0);
        assert_eq!(h1.completion_count(), 4);
    }

    #[test]
    fn in_flight_removal_quarantines_and_forwards_the_late_completion() {
        let l0 = MockLeaf::ready_firing(1);
        let h0 = l0.handle();
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let id0 = rr.add_child(l0);

        // Dispatch, then remove the child while the item is in flight:
        // the subtree is quarantined, not handed back.
        let work = rr.take_next().expect("leaf fires");
        assert!(matches!(rr.remove_child(id0), Some(RemovedChild::Draining)));

        // A replacement child must get a fresh id while the old id is
        // owed a completion, so the completion cannot alias onto it.
        let l1 = MockLeaf::ready_firing(2);
        let h1 = l1.handle();
        let id1 = rr.add_child(l1);
        assert_ne!(id0, id1);

        rr.on_complete(Completion {
            completed_at: Instant::now(),
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        });
        assert_eq!(h1.completion_count(), 0, "not misrouted to the new child");
        assert_eq!(
            h0.completion_count(),
            1,
            "forwarded into the quarantined subtree: exactly-once holds"
        );

        // Fully drained: the id is now recyclable.
        let id2 = rr.add_child(MockLeaf::ready_firing(3));
        assert_eq!(id2, id0);
    }

    #[test]
    fn idle_removal_detaches_and_recycles_the_id_immediately() {
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let id0 = rr.add_child(MockLeaf::ready_firing(1));
        // Nothing in flight: the subtree is handed back for reuse and
        // the id frees at once, so unbounded add/remove churn cannot
        // exhaust the u32 tag space.
        assert!(matches!(
            rr.remove_child(id0),
            Some(RemovedChild::Detached(_))
        ));
        let id1 = rr.add_child(MockLeaf::ready_firing(2));
        assert_eq!(id1, id0);
    }

    #[test]
    fn completion_for_a_live_child_decrements_its_in_flight_count() {
        // Remove after the completion has already drained: the id must
        // free immediately (the slot's count went back to zero).
        let l0 = MockLeaf::ready_firing(1);
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let id0 = rr.add_child(l0);
        dispatch_and_complete(&mut rr, CompletionOutcome::Succeeded, Duration::ZERO)
            .expect("leaf fires");
        assert!(rr.remove_child(id0).is_some());
        let id1 = rr.add_child(MockLeaf::ready_firing(2));
        assert_eq!(id1, id0);
    }

    #[test]
    fn idle_churn_keeps_the_rotation_queue_bounded() {
        // Add/remove churn with no take_next in between must not grow
        // the queue: stale entries are swept once they outnumber the
        // live children.
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        rr.add_child(MockLeaf::ready_firing(1));
        for _ in 0..1_000 {
            let id = rr.add_child(MockLeaf::ready_firing(2));
            assert!(rr.remove_child(id).is_some());
        }
        assert_eq!(rr.len(), 1);
        assert!(
            rr.queue_len() <= 2 * rr.len() + 1,
            "queue grew to {} entries",
            rr.queue_len()
        );
    }

    #[test]
    fn update_ready_skips_freed_slots_after_mass_removal() {
        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        let keep = rr.add_child(MockLeaf::ready_firing(1));
        let ids: Vec<u32> = (0..100)
            .map(|_| rr.add_child(MockLeaf::not_ready(None)))
            .collect();
        for id in ids {
            assert!(rr.remove_child(id).is_some());
        }
        assert_eq!(rr.len(), 1);
        assert!(rr.update_ready(Instant::now()).ready);
        let work = rr.take_next().expect("surviving child fires");
        assert_eq!(work.routing.depth(), 1);
        drop(work);
        let _ = keep;
    }

    #[test]
    fn completion_routes_back_to_the_originating_child() {
        let l0 = MockLeaf::ready_idle(); // does not fire
        let l1 = MockLeaf::ready_firing(42); // fires
        let h0 = l0.handle();
        let h1 = l1.handle();

        let mut rr: RoundRobin<u64, ()> = RoundRobin::new();
        rr.add_child(l0);
        rr.add_child(l1);

        dispatch_and_complete(
            &mut rr,
            CompletionOutcome::Succeeded,
            Duration::from_millis(7),
        )
        .expect("l1 should fire");

        // The completion must have landed on l1, not l0.
        assert_eq!(h0.completion_count(), 0);
        assert_eq!(h1.completion_count(), 1);
        assert_eq!(
            h1.last_completion_outcome(),
            Some(CompletionOutcome::Succeeded)
        );
    }
}
