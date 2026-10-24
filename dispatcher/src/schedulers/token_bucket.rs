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

//! Token-bucket rate-limiting scheduler.
//!
//! Wraps a single child whose meta exposes [`HasCost`]. Tokens accrue
//! continuously at `refill_amount` per `refill_interval` up to
//! `capacity`. Admission requires the balance to cover the child's
//! projected next cost ([`Readiness::next_cost`]); without a hint, one
//! whole token; a cost above `capacity` is gated at a full bucket. On
//! dispatch the item's *actual* meta cost is charged, which may drive
//! the balance negative (debt) — the deficit is paid back before the
//! next admission.
//!
//! While the balance is insufficient, `update_ready` reports the
//! instant at which it will suffice. Accounting is exact integer math,
//! scaled by `refill_interval` in nanoseconds.

use core::convert::TryFrom as _;
use core::marker::PhantomData;
use core::time::Duration;
use std::time::Instant;

use crate::scheduler::{ScheduledWork, Scheduler};
use crate::work::{Completion, CostUnits, HasCost as _, Readiness};

/// Configuration for a [`TokenBucket`].
#[derive(Debug, Clone, Copy)]
pub struct TokenBucketConfig {
    /// Maximum stored tokens (the burst size). The bucket starts full.
    pub capacity: CostUnits,
    /// Tokens added per `refill_interval`. Zero means the bucket never
    /// refills: only the initial `capacity` is ever spent.
    pub refill_amount: CostUnits,
    /// Period over which `refill_amount` accrues. Accrual is continuous,
    /// not stepped. A zero interval means an unlimited rate (the bucket
    /// is always full).
    pub refill_interval: Duration,
}

impl Default for TokenBucketConfig {
    fn default() -> Self {
        Self {
            capacity: CostUnits::new(16),
            refill_amount: CostUnits::new(1),
            refill_interval: Duration::from_secs(1),
        }
    }
}

/// Token-bucket decorator wrapping a single cost-aware child scheduler.
///
/// Children whose meta does not implement [`crate::HasCost`] can be
/// adapted with [`crate::schedulers::FixedCost`].
pub struct TokenBucket<T, C: Scheduler<T>> {
    inner: C,
    cfg: TokenBucketConfig,
    /// `cfg.refill_interval` in nanoseconds, cached: this sits on the
    /// per-readiness-pass hot path.
    interval_nanos: u64,
    /// `scale(cfg.capacity)`, cached for the same reason.
    scaled_capacity: i128,
    /// Balance scaled by `refill_interval` nanoseconds: one token equals
    /// `refill_interval.as_nanos()` scaled units, so accrual per elapsed
    /// nanosecond is exactly `refill_amount` scaled units.
    scaled_balance: i128,
    last_refill: Instant,
    last_now: Instant,
    /// Admission gate captured at `update_ready`, re-checked in
    /// `take_next` because branches may pull without a fresh readiness
    /// pass.
    admission_cost: CostUnits,
    _t: PhantomData<fn() -> T>,
}

impl<T, C> TokenBucket<T, C>
where
    C: Scheduler<T>,
    C::Meta: crate::HasCost,
{
    /// Create a new [`TokenBucket`] with the given config and child
    /// scheduler. The bucket starts at full `capacity`; `now` is the
    /// accounting epoch — pass the driving clock's current time (e.g.
    /// [`crate::ManualClock::now`] under a manual-clock runtime).
    ///
    /// The child's meta must expose [`crate::HasCost`] so the actual
    /// cost of each dispatched item can be charged; adapt cost-naive
    /// children with [`crate::schedulers::FixedCost`].
    #[must_use]
    pub fn new(now: Instant, cfg: TokenBucketConfig, child: C) -> Self {
        let mut bucket = Self {
            inner: child,
            cfg,
            interval_nanos: 0,
            scaled_capacity: 0,
            scaled_balance: 0,
            last_refill: now,
            last_now: now,
            admission_cost: CostUnits::new(1),
            _t: PhantomData,
        };
        bucket.recache();
        bucket.scaled_balance = bucket.scaled_capacity;
        bucket
    }

    /// Recompute the fields derived from `cfg`; must run after every
    /// `cfg` change.
    fn recache(&mut self) {
        self.interval_nanos =
            u64::try_from(self.cfg.refill_interval.as_nanos()).unwrap_or(u64::MAX);
        self.scaled_capacity =
            i128::from(self.cfg.capacity.get()).saturating_mul(i128::from(self.interval_nanos));
    }

    /// Currently available whole tokens. Negative while the bucket is in
    /// debt from an item whose actual cost exceeded the admission
    /// estimate.
    #[must_use]
    pub fn available(&self) -> i64 {
        let interval = i128::from(self.interval_nanos);
        if interval == 0 {
            return cost_to_i64(self.cfg.capacity);
        }
        tokens_to_i64(self.scaled_balance.div_euclid(interval))
    }

    /// Replace the bucket parameters, preserving the current balance
    /// (clamped to the new capacity).
    pub fn set_config(&mut self, cfg: TokenBucketConfig) {
        // Convert the balance to the new interval scale before swapping.
        let old_interval = i128::from(self.interval_nanos);
        let tokens = if old_interval == 0 {
            i128::from(cost_to_i64(self.cfg.capacity))
        } else {
            self.scaled_balance.div_euclid(old_interval)
        };
        self.cfg = cfg;
        self.recache();
        self.scaled_balance = tokens
            .saturating_mul(i128::from(self.interval_nanos))
            .min(self.scaled_capacity);
    }

    /// Current configuration.
    #[must_use]
    pub const fn config(&self) -> TokenBucketConfig {
        self.cfg
    }

    fn scale(&self, cost: CostUnits) -> i128 {
        i128::from(cost.get()).saturating_mul(i128::from(self.interval_nanos))
    }

    fn refill(&mut self, now: Instant) {
        if self.interval_nanos == 0 {
            // Unlimited rate: always full.
            self.scaled_balance = self.scaled_capacity;
            self.last_refill = now;
            return;
        }
        // The runtime issues one full readiness pass per admitted item,
        // all at the same instant: make the repeats a compare-and-skip.
        if now <= self.last_refill {
            return;
        }
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        let accrued = i128::try_from(elapsed.as_nanos())
            .unwrap_or(i128::MAX)
            .saturating_mul(i128::from(self.cfg.refill_amount.get()));
        self.scaled_balance = self
            .scaled_balance
            .saturating_add(accrued)
            .min(self.scaled_capacity);
    }

    fn covers(&self, cost: CostUnits) -> bool {
        self.scaled_balance >= self.scale(cost)
    }

    fn spend(&mut self, cost: CostUnits) {
        self.scaled_balance = self.scaled_balance.saturating_sub(self.scale(cost));
    }

    /// Instant at which the balance will cover `cost`, or `None` when it
    /// never will (zero refill rate, or an ETA beyond `Instant` range).
    fn covered_at(&self, cost: CostUnits) -> Option<Instant> {
        let deficit = self.scale(cost).saturating_sub(self.scaled_balance);
        if deficit <= 0 {
            return Some(self.last_refill);
        }
        let amount = i128::from(self.cfg.refill_amount.get());
        if amount == 0 || self.interval_nanos == 0 {
            return None;
        }

        // Ceiling division; both operands are positive here.
        let nanos = deficit.saturating_add(amount - 1) / amount;
        let nanos = u64::try_from(nanos).unwrap_or(u64::MAX);
        self.last_refill.checked_add(Duration::from_nanos(nanos))
    }
}

impl<T, C> Scheduler<T> for TokenBucket<T, C>
where
    T: Send + 'static,
    C: Scheduler<T>,
    C::Meta: crate::HasCost,
{
    type Meta = C::Meta;

    fn update_ready(&mut self, now: Instant) -> Readiness {
        self.last_now = now;
        self.refill(now);
        // In debt no admission gate can be covered: skip the subtree
        // walk and wake when the last-known gate would be covered again
        // (the recovery pass recomputes the real gate; an early wake
        // just re-checks). Without a finite recovery ETA fall through
        // to the full walk, so the child's own wake hint and clocks are
        // preserved.
        if self.scaled_balance < 0 {
            if let Some(eta) = self.covered_at(self.admission_cost) {
                return Readiness::not_ready(Some(eta));
            }
        }
        let child = self.inner.update_ready(now);
        if !child.ready {
            return child;
        }

        let cost = child.next_cost.unwrap_or(CostUnits::new(1));
        // A cost beyond capacity can never be covered (the balance
        // clamps at capacity): gate it at "bucket full", overshoot
        // becomes debt.
        let gate = CostUnits::new(cost.get().min(self.cfg.capacity.get()));
        self.admission_cost = gate;
        if self.covers(gate) {
            Readiness {
                ready: true,
                next_update_at: child.next_update_at,
                next_cost: Some(cost),
            }
        } else {
            Readiness::not_ready(self.covered_at(gate))
        }
    }

    fn take_next(&mut self) -> Option<ScheduledWork<T, C::Meta>> {
        self.refill(self.last_now);
        // admission_cost may predate a debt period (the pre-gate in
        // update_ready returns without refreshing it); that is safe
        // because a negative balance covers nothing, and any pass with
        // a non-negative balance recomputes the gate before this check
        // can pass.
        if !self.covers(self.admission_cost) {
            return None;
        }
        let work = self.inner.take_next()?;
        self.spend(work.meta.cost());
        Some(work)
    }

    fn on_complete(&mut self, completion: Completion<C::Meta>) {
        self.inner.on_complete(completion);
    }

    fn register_queue_event_sink(&mut self, sink: crate::QueueEventSink) {
        self.inner.register_queue_event_sink(sink);
    }
}

fn cost_to_i64(cost: CostUnits) -> i64 {
    i64::try_from(cost.get()).unwrap_or(i64::MAX)
}

/// Clamp an i128 token count to i64, preserving the sign.
fn tokens_to_i64(tokens: i128) -> i64 {
    i64::try_from(tokens).unwrap_or(if tokens < 0 { i64::MIN } else { i64::MAX })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::time::Duration;
    use std::time::Instant;

    use super::{TokenBucket, TokenBucketConfig};
    use crate::scheduler::Scheduler as _;
    use crate::schedulers::tests::MockLeaf;
    use crate::work::{Completion, CompletionOutcome, CostUnits, Readiness, WithCost};

    fn costed_leaf(cost: u64) -> MockLeaf<WithCost<()>> {
        MockLeaf::new(
            WithCost::new((), CostUnits::new(cost)),
            Readiness::ready(Some(CostUnits::new(cost))),
            Some(1),
        )
    }

    fn cfg(capacity: u64, amount: u64, interval: Duration) -> TokenBucketConfig {
        TokenBucketConfig {
            capacity: CostUnits::new(capacity),
            refill_amount: CostUnits::new(amount),
            refill_interval: interval,
        }
    }

    fn drain<T, C>(tb: &mut TokenBucket<T, C>)
    where
        T: Send + 'static,
        C: crate::Scheduler<T>,
        C::Meta: crate::HasCost,
    {
        while let Some(work) = tb.take_next() {
            tb.on_complete(Completion {
                completed_at: tb.last_now,
                outcome: CompletionOutcome::Succeeded,
                latency: Duration::ZERO,
                meta: work.meta,
                routing: work.routing,
            });
        }
    }

    #[test]
    fn burst_up_to_capacity_then_blocks() {
        let leaf = costed_leaf(1);
        let handle = leaf.handle();
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(3, 1, Duration::from_secs(1)), leaf);

        assert!(tb.update_ready(t0).ready);
        drain(&mut tb);
        assert_eq!(handle.take_next_count(), 3);
        assert_eq!(tb.available(), 0);

        // Exhausted: not ready, with a refill hint exactly one interval
        // out (the accounting epoch is t0, so no sliver of drift).
        let r = tb.update_ready(t0);
        assert!(!r.ready);
        let eta = r.next_update_at.expect("refill hint");
        assert_eq!(eta, t0 + Duration::from_secs(1));
    }

    #[test]
    fn refills_continuously_over_time() {
        let leaf = costed_leaf(1);
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(4, 2, Duration::from_secs(1)), leaf);
        tb.update_ready(t0);
        drain(&mut tb);
        assert_eq!(tb.available(), 0);

        // 2 tokens/s: after 1.5s exactly 3 tokens have accrued.
        let t1 = t0 + Duration::from_millis(1500);
        assert!(tb.update_ready(t1).ready);
        assert_eq!(tb.available(), 3);
        drain(&mut tb);
        assert_eq!(tb.available(), 0);
    }

    #[test]
    fn balance_clamps_at_capacity() {
        let leaf = costed_leaf(1);
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(2, 10, Duration::from_secs(1)), leaf);
        tb.update_ready(t0 + Duration::from_secs(100));
        assert_eq!(tb.available(), 2);
    }

    #[test]
    fn actual_cost_above_estimate_creates_debt() {
        // Admission hint says 1, actual meta cost is 5.
        let leaf = MockLeaf::new(
            WithCost::new((), CostUnits::new(5)),
            Readiness::ready(Some(CostUnits::new(1))),
            Some(1),
        );
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(2, 1, Duration::from_secs(1)), leaf);
        assert!(tb.update_ready(t0).ready);
        let work = tb.take_next().expect("admitted on the estimate");
        tb.on_complete(Completion {
            completed_at: tb.last_now,
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        });
        // 2 - 5 = -3: in debt. The pre-gate wakes when the last-known
        // gate (1 token) is covered: 3 debt + 1 at 1 token/s.
        assert_eq!(tb.available(), -3);
        let r = tb.update_ready(t0);
        assert!(!r.ready);
        let eta = r.next_update_at.expect("debt hint");
        assert_eq!(eta, t0 + Duration::from_secs(4));
        assert!(tb.update_ready(eta).ready);
    }

    #[test]
    fn no_hint_admits_on_one_token_and_charges_actual_cost() {
        // Ready with no next_cost hint, but the meta carries cost 5.
        let leaf = MockLeaf::new(
            WithCost::new((), CostUnits::new(5)),
            Readiness::ready(None),
            Some(1),
        );
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(2, 1, Duration::from_secs(1)), leaf);

        assert!(tb.update_ready(t0).ready, "one token suffices to admit");
        let work = tb.take_next().expect("admitted without a hint");
        tb.on_complete(Completion {
            completed_at: tb.last_now,
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        });
        // The actual meta cost was charged: 2 - 5 = -3.
        assert_eq!(tb.available(), -3);
        assert!(!tb.update_ready(t0).ready, "in debt: blocked");
    }

    #[test]
    fn cost_beyond_capacity_admits_at_full_bucket_instead_of_stalling() {
        // Hinted cost 5 with capacity 2: coverage is unreachable, so the
        // gate clamps to "bucket full" and the item runs on debt.
        let leaf = costed_leaf(5);
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(2, 1, Duration::from_secs(1)), leaf);

        assert!(tb.update_ready(t0).ready, "full bucket admits");
        let work = tb.take_next().expect("dispatched at full burst");
        tb.on_complete(Completion {
            completed_at: tb.last_now,
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        });
        assert_eq!(tb.available(), -3, "2 - 5: overshoot became debt");

        // Blocked while paying the debt back, with a *reachable* ETA
        // (debt 3 + last-known gate 2 at 1 token/s), then admitted
        // again — no forever-receding ETA livelock.
        let r = tb.update_ready(t0);
        assert!(!r.ready);
        let eta = r.next_update_at.expect("debt hint");
        assert_eq!(eta, t0 + Duration::from_secs(5));
        assert!(tb.update_ready(eta).ready);
    }

    #[test]
    fn deep_debt_reports_negative_available() {
        // A near-max actual cost must read as i64::MIN, not i64::MAX.
        let leaf = MockLeaf::new(
            WithCost::new((), CostUnits::new(u64::MAX)),
            Readiness::ready(None),
            Some(1),
        );
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(2, 1, Duration::from_secs(1)), leaf);
        tb.update_ready(t0);
        let work = tb.take_next().expect("admitted on the 1-token gate");
        tb.on_complete(Completion {
            completed_at: tb.last_now,
            outcome: CompletionOutcome::Succeeded,
            latency: Duration::ZERO,
            meta: work.meta,
            routing: work.routing,
        });
        assert!(tb.available() < 0, "deep debt must not read as positive");
    }

    #[test]
    fn take_next_gates_without_fresh_update_ready() {
        let leaf = costed_leaf(1);
        let handle = leaf.handle();
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(1, 1, Duration::from_secs(1)), leaf);
        tb.update_ready(t0);
        assert!(tb.take_next().is_some());
        // Second pull without update_ready must be refused, and must not
        // reach the child.
        let calls_before = handle.take_next_count();
        assert!(tb.take_next().is_none());
        assert_eq!(handle.take_next_count(), calls_before);
    }

    #[test]
    fn huge_elapsed_and_refill_amount_saturate_instead_of_panicking() {
        let leaf = costed_leaf(1);
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(3, u64::MAX, Duration::from_secs(1)), leaf);
        tb.update_ready(t0);
        drain(&mut tb);

        // ~317k years of accrual at u64::MAX tokens/s: the i128 product
        // saturates and the balance clamps at capacity.
        let t1 = t0 + Duration::from_secs(10_000_000_000_000);
        assert!(tb.update_ready(t1).ready);
        assert_eq!(tb.available(), 3);
    }

    #[test]
    fn debt_without_recovery_eta_still_propagates_the_child_hint() {
        // Zero refill: debt never recovers, so the pre-gate has no ETA
        // and must fall through to the child walk — the child's finite
        // wake hint reaches the runtime instead of a hint-less park.
        let t0 = Instant::now();
        let child_due = t0 + Duration::from_secs(7);
        let leaf: MockLeaf<WithCost<()>> = MockLeaf::new(
            WithCost::new((), CostUnits::new(5)),
            Readiness::not_ready(Some(child_due)),
            None,
        );
        let mut tb = TokenBucket::new(t0, cfg(1, 0, Duration::from_secs(1)), leaf);
        tb.spend(CostUnits::new(5));
        assert!(tb.available() < 0);

        let r = tb.update_ready(t0);
        assert!(!r.ready);
        assert_eq!(r.next_update_at, Some(child_due));
    }

    #[test]
    fn zero_refill_rate_never_recovers() {
        let leaf = costed_leaf(1);
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(1, 0, Duration::from_secs(1)), leaf);
        tb.update_ready(t0);
        drain(&mut tb);
        let r = tb.update_ready(t0 + Duration::from_secs(9999));
        assert!(!r.ready);
        assert!(r.next_update_at.is_none(), "no ETA when rate is zero");
    }

    #[test]
    fn zero_interval_is_unlimited() {
        let leaf = costed_leaf(1);
        let handle = leaf.handle();
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(2, 1, Duration::ZERO), leaf);
        for _ in 0..10 {
            assert!(tb.update_ready(t0).ready);
            let work = tb.take_next().expect("always admitted");
            tb.on_complete(Completion {
                completed_at: tb.last_now,
                outcome: CompletionOutcome::Succeeded,
                latency: Duration::ZERO,
                meta: work.meta,
                routing: work.routing,
            });
        }
        assert_eq!(handle.take_next_count(), 10);
    }

    #[test]
    fn set_config_preserves_balance_and_clamps() {
        let leaf = costed_leaf(1);
        let t0 = Instant::now();
        let mut tb = TokenBucket::new(t0, cfg(10, 1, Duration::from_secs(1)), leaf);
        tb.update_ready(t0);
        assert_eq!(tb.available(), 10);
        tb.set_config(cfg(4, 2, Duration::from_millis(500)));
        assert_eq!(tb.available(), 4, "clamped to the new capacity");
    }

    #[test]
    fn child_not_ready_passes_through() {
        let leaf: MockLeaf<WithCost<()>> = MockLeaf::new(
            WithCost::new((), CostUnits::ZERO),
            Readiness::not_ready(None),
            None,
        );
        let now = Instant::now();
        let mut tb = TokenBucket::new(now, cfg(3, 1, Duration::from_secs(1)), leaf);
        assert!(!tb.update_ready(now).ready);
    }
}
