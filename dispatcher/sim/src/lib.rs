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

//! Virtual-time simulation harness shared by the integration tests.
//!
//! Per-source subtree under a shared round-robin root:
//!
//! ```text
//! RoundRobin (root)
//! └─ CircuitBreaker
//!    └─ TokenBucket
//!       └─ RoundRobin — one FixedCost(PeriodicLeaf) per entry in TASKS
//! ```
//!
//! Work completes immediately (success, or failure while the source's
//! flag is set) and the clock is manual: the driver jumps to every
//! `SleepUntil` target instead of sleeping, so runs are deterministic
//! and identical on any host.

use core::convert::TryFrom as _;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use core::time::Duration;
use std::num::NonZeroUsize;
use std::ops::Range;
use std::sync::Arc;
use std::time::Instant;

use nv_redfish_dispatcher::{
    CircuitBreaker, CircuitBreakerConfig, ClockConfig, Completion, CostUnits, FixedCost,
    FutureWork, ManualClock, PeriodicLeaf, QueueEventSink, Readiness, RoundRobin, Runtime,
    RuntimeConfig, RuntimeOutput, ScheduledWork, Scheduler, TokenBucket, TokenBucketConfig,
    WithCost,
};

/// Successful work reports (source, task).
pub type Ev = (u32, u8);
/// Failed work reports the source as the error.
pub type Err = u32;
/// Work payload executed by the simulated runtime.
pub type Work = FutureWork<Ev, Err>;
/// Meta produced by the per-source subtrees.
pub type Meta = WithCost<()>;

/// Periodic task: fires every `interval`, costing `cost` units.
#[derive(Clone, Copy)]
pub struct Task {
    pub id: u8,
    pub interval: Duration,
    pub cost: u64,
}

/// Mixed profile: cheap-frequent, mid, expensive-rare. Total demand
/// 1/10 + 6/30 + 30/300 = 0.4 units/s per source.
pub const TASKS: [Task; 3] = [
    Task {
        id: 0,
        interval: Duration::from_secs(10),
        cost: 1,
    },
    Task {
        id: 1,
        interval: Duration::from_secs(30),
        cost: 6,
    },
    Task {
        id: 2,
        interval: Duration::from_secs(300),
        cost: 30,
    },
];

/// Fires at t = 0, interval, 2·interval, …: count within `window`.
pub fn expected_fires(task: Task, window: Duration) -> u64 {
    assert!(!task.interval.is_zero(), "task interval must be non-zero");
    u64::try_from(window.as_nanos().div_ceil(task.interval.as_nanos())).unwrap_or(u64::MAX)
}

/// Dispatches one unthrottled source produces within `window`.
pub fn expected_dispatches(window: Duration) -> u64 {
    TASKS.iter().map(|t| expected_fires(*t, window)).sum()
}

/// Cost of the task with the given id (0 for unknown ids).
pub fn cost_of(task: u8) -> u64 {
    TASKS.iter().find(|t| t.id == task).map_or(0, |t| t.cost)
}

/// 1 unit/s with a burst covering the t=0 wave: the task intervals, not
/// the bucket, set the pace.
pub fn ample_bucket() -> TokenBucketConfig {
    TokenBucketConfig {
        capacity: CostUnits::new(40),
        refill_amount: CostUnits::new(1),
        refill_interval: Duration::from_secs(1),
    }
}

/// 0.2 units/s against 0.4 units/s of demand: the bucket sets the pace.
pub fn scarce_bucket() -> TokenBucketConfig {
    TokenBucketConfig {
        capacity: CostUnits::new(4),
        refill_amount: CostUnits::new(1),
        refill_interval: Duration::from_secs(5),
    }
}

/// Breaker used by every [`source`].
pub fn breaker() -> CircuitBreakerConfig {
    CircuitBreakerConfig {
        failure_threshold: 0.5,
        sample_window: 8,
        min_samples: 4,
        cool_down: Duration::from_secs(30),
    }
}

/// Per-source subtree built at `now`. While `fail` is set every dispatch
/// reports a failure.
pub fn source(
    now: Instant,
    idx: u32,
    bucket: TokenBucketConfig,
    fail: Arc<AtomicBool>,
) -> impl Scheduler<Work, Meta = Meta> {
    source_due_at(now, now, idx, bucket, fail)
}

/// [`source`] whose tasks first fire at `first_due` instead of `now`.
pub fn source_due_at(
    now: Instant,
    first_due: Instant,
    idx: u32,
    bucket: TokenBucketConfig,
    fail: Arc<AtomicBool>,
) -> impl Scheduler<Work, Meta = Meta> {
    let mut tasks: RoundRobin<Work, Meta> = RoundRobin::new();
    for task in TASKS {
        let fail = fail.clone();
        let leaf = PeriodicLeaf::starting_at(now, first_due, task.interval, move || {
            let fail = fail.clone();
            Box::pin(async move {
                if fail.load(Ordering::Relaxed) {
                    Err(idx)
                } else {
                    Ok(vec![(idx, task.id)])
                }
            }) as Work
        });
        tasks.add_child(FixedCost::new(CostUnits::new(task.cost), leaf));
    }
    CircuitBreaker::new(breaker(), TokenBucket::new(now, bucket, tasks))
}

/// Add one [`source`] per id in `ids`, all sharing `fail`.
pub fn add_sources(
    root: &mut RoundRobin<Work, Meta>,
    now: Instant,
    ids: Range<u32>,
    bucket: TokenBucketConfig,
    fail: &Arc<AtomicBool>,
) {
    for idx in ids {
        root.add_child(source(now, idx, bucket, fail.clone()));
    }
}

/// Scheduler-trait entry counters shared by every [`Counted`] wrapper.
#[derive(Default)]
pub struct OpCounts {
    /// `update_ready` entries.
    pub update_ready: AtomicU64,
    /// `take_next` entries.
    pub take_next: AtomicU64,
    /// `on_complete` entries.
    pub on_complete: AtomicU64,
}

/// Pass-through decorator counting trait entries into a shared
/// [`OpCounts`].
pub struct Counted<S> {
    inner: S,
    counts: Arc<OpCounts>,
}

impl<S> Counted<S> {
    /// Wrap `inner`, tallying its trait entries into `counts`.
    pub fn new(counts: Arc<OpCounts>, inner: S) -> Self {
        Self { inner, counts }
    }
}

impl<T, S> Scheduler<T> for Counted<S>
where
    T: Send + 'static,
    S: Scheduler<T>,
{
    type Meta = S::Meta;

    fn update_ready(&mut self, now: Instant) -> Readiness {
        self.counts.update_ready.fetch_add(1, Ordering::Relaxed);
        self.inner.update_ready(now)
    }

    fn take_next(&mut self) -> Option<ScheduledWork<T, S::Meta>> {
        self.counts.take_next.fetch_add(1, Ordering::Relaxed);
        self.inner.take_next()
    }

    fn on_complete(&mut self, completion: Completion<S::Meta>) {
        self.counts.on_complete.fetch_add(1, Ordering::Relaxed);
        self.inner.on_complete(completion);
    }

    fn register_queue_event_sink(&mut self, sink: QueueEventSink) {
        self.inner.register_queue_event_sink(sink);
    }
}

/// One observed dispatch: virtual time since start, source, task
/// (`u8::MAX` for failures), outcome.
pub struct Dispatch {
    /// Virtual time since the start of the run.
    pub at: Duration,
    /// Originating source.
    pub source: u32,
    /// Originating task (`u8::MAX` for failures).
    pub task: u8,
    /// Whether the work succeeded.
    pub ok: bool,
}

/// Dispatches in `log` matching `f`.
pub fn count(log: &[Dispatch], f: impl Fn(&Dispatch) -> bool) -> u64 {
    log.iter().filter(|d| f(d)).count() as u64
}

/// Every task of every source in `ids` fired exactly on schedule.
pub fn assert_interval_exact(log: &[Dispatch], ids: Range<u32>, window: Duration) {
    for idx in ids {
        for task in TASKS {
            assert_eq!(
                count(log, |d| d.source == idx && d.task == task.id),
                expected_fires(task, window),
                "source {} task {}",
                idx,
                task.id
            );
        }
    }
}

/// Drive `root` for `window` of virtual time under `clock` (the clock
/// whose `now()` built the tree). `actions` run at their scheduled
/// virtual instants, which must lie within the window. Returns the
/// dispatch log.
pub async fn simulate(
    clock: ManualClock,
    root: RoundRobin<Work, Meta>,
    window: Duration,
    mut actions: Vec<(Duration, Box<dyn FnOnce() + Send>)>,
) -> Vec<Dispatch> {
    actions.sort_by_key(|(offset, _)| *offset);
    let mut runtime: Runtime<Ev, Err, Meta> = Runtime::new(
        RuntimeConfig {
            global_max_in_flight: NonZeroUsize::new(256).expect("non-zero"),
            clock: ClockConfig::Manual(clock.clone()),
        },
        root,
    );
    let handle = runtime.handle();
    let start = clock.now();
    let end = start + window;

    let mut actions = actions.into_iter().peekable();
    let mut log = Vec::new();
    loop {
        match runtime.next().await {
            RuntimeOutput::Work { result, .. } => {
                let at = clock.now().duration_since(start);
                match result {
                    Ok(events) => log.extend(events.into_iter().map(|(source, task)| Dispatch {
                        at,
                        source,
                        task,
                        ok: true,
                    })),
                    Err(source) => log.push(Dispatch {
                        at,
                        source,
                        task: u8::MAX,
                        ok: false,
                    }),
                }
            }
            RuntimeOutput::SleepUntil(t) => {
                let due_action = actions
                    .peek()
                    .is_some_and(|(offset, _)| start + *offset <= t);
                if due_action {
                    let (offset, action) = actions.next().expect("peeked");
                    clock.advance_to(start + offset);
                    action();
                } else if t >= end {
                    handle.graceful_shutdown();
                } else {
                    clock.advance_to(t);
                }
            }
            RuntimeOutput::Runtime(_) => {}
            RuntimeOutput::Shutdown => break,
        }
    }
    assert!(
        actions.peek().is_none(),
        "scheduled actions were never reached within the window"
    );
    log
}
