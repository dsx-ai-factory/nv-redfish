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

//! Generic dispatcher runtime.
//!
//! [`Runtime::next`] is the single ordered execution and output interface.
//! Each call drains the oldest queued output if one exists; otherwise it may
//! fill in-flight work up to [`RuntimeConfig::global_max_in_flight`], poll the
//! in-flight set, queue completed work outputs, and return once an output is
//! available. If no progress is possible, it parks.
//!
//! The runtime is policy-free and meta-blind. All scheduling lives in the
//! root [`Scheduler`] subtree; the runtime only dispatches payloads, polls
//! the in-flight set, forwards completions back through the tree, enforces
//! the runtime-wide caps in [`RuntimeConfig`], and emits [`RuntimeOutput`].
//!
//! The root sits behind an internal mutex. The driver and
//! [`RuntimeHandle::with_root`] / [`RuntimeHandle::with_root_mut`] share
//! the same lock, so user mutations and driver steps serialize naturally.

use crate::scheduler::private::SchedulerObj;
use crate::scheduler::Scheduler;
use crate::schedulers::{RemovedChild, RoundRobin, StrictPriority};
use crate::stats::OutputQueueStats;
use crate::stats::RuntimeStats;
use crate::work::WorkMeta;
use crate::Completion;
use crate::CompletionOutcome;
use crate::RoutingPath;
use crate::RuntimeEventType;
use crate::ScheduledWork;
use core::convert::TryFrom as _;
use core::future::Future;
use core::marker::PhantomData;
use core::pin::Pin;
use core::sync::atomic::AtomicU64;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering;
use core::task::Context;
use core::task::Poll;
use core::time::Duration;
use futures_core::Stream as _;
use futures_util::stream::FuturesUnordered;
use std::collections::VecDeque;
use std::mem;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Waker;
use std::time::Instant;

/// `PhantomData` alias for the runtime's three type parameters; factored
/// to keep struct types simple under `clippy::type_complexity`.
type RuntimePhantom<Ev, Err, M> = PhantomData<fn() -> (Ev, Err, M)>;

#[derive(Clone, Copy)]
enum SleepHint {
    /// Deadline observed from the scheduler but not yet returned to the driver.
    Observed(Instant),

    /// Deadline already returned to the driver.
    Emitted(Instant),
}

impl SleepHint {
    const fn deadline(self) -> Instant {
        match self {
            Self::Observed(deadline) | Self::Emitted(deadline) => deadline,
        }
    }
}

/// Work payload consumed by this runtime: a boxed future with terminal
/// value `Result<Vec<Ev>, Err>`. Schedulers parameterized as
/// `Scheduler<FutureWork<Ev, Err>>` are compatible with [`Runtime::new`].
///
/// [`crate::Scheduler`] is generic over the payload and never inspects it,
/// so alternate runtimes can pick another shape (sync closures, batched
/// descriptors, …) and reuse the same scheduler types.
pub type FutureWork<Ev, Err> = Pin<Box<dyn Future<Output = Result<Vec<Ev>, Err>> + Send + 'static>>;

/// Generic dispatcher runtime, parameterized by event type `Ev`, error
/// type `Err`, and root meta type `M`.
///
/// `M` is whatever the root scheduler exposes as `Self::Meta` — typically
/// a stack of wrappers like `WithPriority<WithCost<()>>`.
///
/// Not `Clone`: only one consumer drives [`Runtime::next`]. Use
/// [`Runtime::handle`] for cloneable control handles.
pub struct Runtime<Ev, Err, M: WorkMeta> {
    config: RuntimeConfig,
    clock: RuntimeClock,
    sleep_hint: Option<SleepHint>,
    in_flight: FuturesUnordered<InFlight<Ev, Err, M>>,
    completion: Vec<Completion<M>>,
    output: VecDeque<RuntimeOutput<Ev, Err>>,
    shared: Arc<Mutex<Shared<Ev, Err, M>>>,
    signals: Arc<RuntimeSignals>,
    stats: Arc<StatsCells>,
    _phantom: RuntimePhantom<Ev, Err, M>,
}

impl<Ev, Err, M> Runtime<Ev, Err, M>
where
    Ev: Send + 'static,
    Err: Send + 'static,
    M: WorkMeta,
{
    /// Build a runtime with the given configuration and root scheduler.
    ///
    /// The bound `S: Scheduler<FutureWork<Ev, Err>, Meta = M>` ties the
    /// tree's payload to the shape this runtime executes. The root is
    /// consumed and stored behind a mutex; reach into it later with
    /// [`RuntimeHandle::with_root`] / [`RuntimeHandle::with_root_mut`] by
    /// supplying the same concrete type for the downcast.
    ///
    /// A blanket `impl Scheduler for Box<S>` lets you pass an existing
    /// `Box<dyn Scheduler<FutureWork<Ev, Err>, Meta = M>>` directly.
    #[must_use]
    pub fn new<S>(config: RuntimeConfig, root: S) -> Self
    where
        S: Scheduler<FutureWork<Ev, Err>, Meta = M>,
    {
        let signals = Arc::new(RuntimeSignals::default());
        let root = {
            let mut root = root;
            root.register_queue_event_sink(signals.queue_event_sink());
            root
        };
        Self {
            clock: match config.clock {
                ClockConfig::Wallclock => RuntimeClock::Wallclock,
                ClockConfig::Virtual(increment) => RuntimeClock::Virtual {
                    now: Instant::now(),
                    increment,
                },
                ClockConfig::Manual(ref clock) => RuntimeClock::Manual(clock.clone()),
            },
            in_flight: FuturesUnordered::new(),
            sleep_hint: None,
            completion: Vec::new(),
            output: VecDeque::new(),
            config,
            shared: Mutex::new(Shared {
                root: Box::new(root),
                shutdown: false,
                _phantom: PhantomData,
            })
            .into(),
            signals,
            stats: Arc::new(StatsCells::default()),
            _phantom: PhantomData,
        }
    }

    /// Cloneable handle for synchronous control and typed root access.
    #[must_use]
    pub fn handle(&self) -> RuntimeHandle<Ev, Err, M> {
        RuntimeHandle {
            shared: self.shared.clone(),
            signals: self.signals.clone(),
            stats: self.stats.clone(),
        }
    }

    /// Handle to the manual clock; `None` unless configured with
    /// [`ClockConfig::Manual`].
    #[must_use]
    pub fn manual_clock(&self) -> Option<ManualClock> {
        match &self.clock {
            RuntimeClock::Manual(clock) => Some(clock.clone()),
            RuntimeClock::Wallclock | RuntimeClock::Virtual { .. } => None,
        }
    }

    /// Poll the in-flight set, converting finished payloads into pending
    /// completions and `Work` outputs. Returns `true` if anything
    /// finished.
    fn drain_completed(
        &mut self,
        in_flight: &mut FuturesUnordered<InFlight<Ev, Err, M>>,
        cx: &mut Context<'_>,
        now: Instant,
    ) -> bool {
        let mut progress = false;
        while let Poll::Ready(Some(completed)) = Pin::new(&mut *in_flight).poll_next(cx) {
            let CompletedWork {
                start,
                meta,
                result,
                routing,
            } = completed;
            let latency = now.duration_since(start);
            progress = true;
            self.completion.push(Completion {
                latency,
                meta,
                routing,
                outcome: if result.is_ok() {
                    CompletionOutcome::Succeeded
                } else {
                    CompletionOutcome::Failed
                },
            });
            self.output
                .push_back(RuntimeOutput::Work { result, latency });
        }
        progress
    }

    /// Advance until an output is available or no progress is possible.
    ///
    /// Step order:
    ///
    /// 1. Register the current driver waker so any external control or
    ///    queue signal racing with this poll can schedule another poll.
    /// 2. Forward completions, collect runtime events, and drain one queued
    ///    output if any.
    /// 3. If below [`RuntimeConfig::global_max_in_flight`], lock shared
    ///    scheduler/control state briefly and dispatch available work until
    ///    the global cap is reached or the scheduler has no work.
    /// 4. Poll in-flight payloads without holding the shared lock; completed
    ///    payloads enqueue [`RuntimeOutput::Work`].
    /// 5. If no output was queued and no synchronous progress was made, park.
    ///
    /// Shares the root lock with [`RuntimeHandle::with_root_mut`]; both
    /// hold it briefly.
    pub const fn next(&mut self) -> NextFuture<'_, Ev, Err, M> {
        NextFuture { runtime: self }
    }
}

/// Future returned by [`Runtime::next`].
pub struct NextFuture<'r, Ev, Err, M: WorkMeta> {
    // Exclusive borrow enforces the single-driver invariant.
    runtime: &'r mut Runtime<Ev, Err, M>,
}

impl<Ev, Err, M> Future for NextFuture<'_, Ev, Err, M>
where
    Ev: Send + 'static,
    Err: Send + 'static,
    M: WorkMeta,
{
    type Output = RuntimeOutput<Ev, Err>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.runtime.signals.register_waker(cx.waker());
        let mut progress = true;
        while progress {
            if !self.runtime.completion.is_empty() {
                let completions = mem::take(&mut self.runtime.completion);
                let mut shared = self
                    .runtime
                    .shared
                    .lock()
                    .expect("dispatcher runtime mutex is poisoned");
                for completion in completions {
                    shared.root.on_complete(completion);
                }
            }
            #[cfg(feature = "runtime-events")]
            {
                let events = self.runtime.signals.take_events();
                self.runtime
                    .output
                    .extend(events.into_iter().map(RuntimeOutput::Runtime));
            }
            if let Some(output) = self.runtime.output.pop_front() {
                self.runtime
                    .stats
                    .output_queued
                    .store(self.runtime.output.len(), Ordering::Relaxed);
                return Poll::Ready(output);
            }
            progress = false;

            let now = self.runtime.clock.now();
            let mut in_flight = mem::take(&mut self.runtime.in_flight);
            let global_max_in_flight = self.runtime.config.global_max_in_flight;

            // Retain a future scheduler deadline across polls, including while
            // payloads remain in flight. Once reached, drop it so the scheduler
            // can report work that has become eligible.
            let mut sleep_hint = self.runtime.sleep_hint.filter(|hint| now < hint.deadline());
            let mut shutdown = false;
            if in_flight.len() < global_max_in_flight.into() {
                let mut shared = self
                    .runtime
                    .shared
                    .lock()
                    .expect("dispatcher runtime mutex is poisoned");
                shutdown = shared.shutdown;
                while !shutdown && in_flight.len() < global_max_in_flight.into() {
                    match shared.next(now) {
                        SharedNextResult::Work(work) => {
                            self.runtime
                                .stats
                                .dispatched
                                .fetch_add(1, Ordering::Relaxed);
                            in_flight.push(InFlight {
                                start: now,
                                work: Some(work),
                            });
                        }
                        SharedNextResult::SleepUntil(v) => {
                            // A later scheduler result cannot postpone an earlier
                            // retained deadline. A newly observed earlier deadline
                            // must be returned so the driver can adjust its timer.
                            if sleep_hint.is_none_or(|hint| hint.deadline() > v) {
                                sleep_hint = Some(SleepHint::Observed(v));
                            }
                            break;
                        }
                        SharedNextResult::Nothing => {
                            // The scheduler currently has no timed transition, so
                            // any retained deadline is no longer actionable.
                            sleep_hint = None;
                            break;
                        }
                    }
                }
            }

            if shutdown {
                // Deadlines only admit future scheduler work, which shutdown
                // forbids.
                sleep_hint = None;
            }

            self.runtime
                .stats
                .in_flight
                .store(in_flight.len() as u64, Ordering::Relaxed);

            progress |= self.runtime.drain_completed(&mut in_flight, cx, now);
            self.runtime
                .stats
                .in_flight
                .store(in_flight.len() as u64, Ordering::Relaxed);
            self.runtime
                .stats
                .output_queued
                .store(self.runtime.output.len(), Ordering::Relaxed);

            self.runtime.in_flight = in_flight;
            // Payload completion and scheduler timing are independent wake-up
            // sources, so preserve the deadline even when work remains active.
            self.runtime.sleep_hint = sleep_hint;

            // During shutdown, scheduler deadlines are ignored and no new
            // work is admitted. Emit the terminal output only after running
            // work, completions, and queued outputs have drained.
            if shutdown
                && self.runtime.in_flight.is_empty()
                && self.runtime.output.is_empty()
                && self.runtime.completion.is_empty()
            {
                self.runtime.sleep_hint = None;
                return Poll::Ready(RuntimeOutput::Shutdown);
            }

            // Return queued work before a newly observed deadline. Mark the
            // deadline as emitted so the next call can poll in-flight work
            // without returning the same deadline again.
            if self.runtime.output.is_empty() {
                if let Some(SleepHint::Observed(deadline)) = sleep_hint {
                    self.runtime.sleep_hint = Some(SleepHint::Emitted(deadline));
                    return Poll::Ready(RuntimeOutput::SleepUntil(deadline));
                }
            }
        }

        // Any retained deadline has already been returned to the driver. This
        // future now parks until another wake source can make progress.
        Poll::Pending
    }
}

/// Runtime-wide configuration. Per-node policy lives inside each
/// [`Scheduler`]; this struct only carries knobs no node owns.
#[derive(Clone, Debug)]
pub struct RuntimeConfig {
    /// Global cap on in-flight items, applied at dispatch admission
    /// on top of any per-subtree admission a branch enforces.
    pub global_max_in_flight: NonZeroUsize,
    /// Runtime clock configuration.
    pub clock: ClockConfig,
}

/// Runtime clock configuration.
#[derive(Clone, Debug, Default)]
pub enum ClockConfig {
    /// Clock that ticks with real time.
    #[default]
    Wallclock,
    /// Clock that increments on specified duration each time it is
    /// requested.
    Virtual(Duration),
    /// Clock driven by the given [`ManualClock`] handle. Construct the
    /// clock first and build time-holding scheduler nodes with
    /// `clock.now()`, so the tree and the runtime share one epoch.
    Manual(ManualClock),
}

/// Manually advanced clock for [`ClockConfig::Manual`] runtimes.
///
/// Cloneable; time starts at the construction instant and only moves
/// forward via [`ManualClock::advance`] / [`ManualClock::advance_to`].
/// Advancing does not wake the runtime — call [`Runtime::next`] afterwards.
#[derive(Clone, Debug)]
pub struct ManualClock {
    epoch: Instant,
    offset_nanos: Arc<AtomicU64>,
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ManualClock {
    /// Clock whose epoch is the construction instant.
    #[must_use]
    pub fn new() -> Self {
        Self {
            epoch: Instant::now(),
            offset_nanos: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Current virtual time.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.epoch + Duration::from_nanos(self.offset_nanos.load(Ordering::Relaxed))
    }

    /// Move time forward by `by`.
    pub fn advance(&self, by: Duration) {
        let by = u64::try_from(by.as_nanos()).unwrap_or(u64::MAX);
        let _ = self
            .offset_nanos
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |cur| {
                Some(cur.saturating_add(by))
            });
    }

    /// Move time forward to `to`. Targets in the past are ignored: the
    /// clock is monotonic.
    pub fn advance_to(&self, to: Instant) {
        let target =
            u64::try_from(to.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX);
        self.offset_nanos.fetch_max(target, Ordering::Relaxed);
    }
}

/// Cloneable handle to a running [`Runtime`].
///
/// Exposes synchronous control plus typed root access. Mutating ops take
/// the internal lock briefly and never wait on work payloads. The runtime
/// itself is not `Clone`.
pub struct RuntimeHandle<Ev, Err, M: WorkMeta> {
    shared: Arc<Mutex<Shared<Ev, Err, M>>>,
    signals: Arc<RuntimeSignals>,
    stats: Arc<StatsCells>,
}

struct RuntimeMutationContext<T> {
    queue_event_sink: crate::QueueEventSink,
    _payload: PhantomData<fn() -> T>,
}

impl<T> RuntimeMutationContext<T>
where
    T: 'static,
{
    fn register<S>(&self, mut scheduler: S) -> S
    where
        S: Scheduler<T>,
    {
        scheduler.register_queue_event_sink(self.queue_event_sink.clone());
        scheduler
    }
}

/// Exclusive, runtime-aware access to a scheduler root.
///
/// This wrapper deliberately does not expose `&mut R` or implement
/// [`DerefMut`](core::ops::DerefMut). Its mutation methods preserve runtime
/// invariants such as registering every newly attached scheduler subtree
/// with the queue-event sink.
pub struct RuntimeRootMut<'a, R, T> {
    root: &'a mut R,
    mutation: RuntimeMutationContext<T>,
}

impl<R, T> RuntimeRootMut<'_, R, T>
where
    T: Send + 'static,
    R: crate::RuntimeChildContainer<T>,
{
    /// Register and attach one child scheduler with branch-specific arguments.
    ///
    /// Registration visits only `child`; its cost is independent of the
    /// number of children already present in the root.
    pub fn add_child_with<S>(&mut self, child: S, args: R::ChildArgs) -> R::ChildId
    where
        S: Scheduler<T, Meta = R::ChildMeta>,
    {
        self.root.attach_child(self.mutation.register(child), args)
    }
}

impl<R, T> RuntimeRootMut<'_, R, T>
where
    T: Send + 'static,
    R: crate::RuntimeChildContainer<T, ChildArgs = ()>,
{
    /// Register and attach one child scheduler without branch-specific arguments.
    ///
    /// Registration visits only `child`; its cost is independent of the
    /// number of children already present in the root.
    pub fn add_child<S>(&mut self, child: S) -> R::ChildId
    where
        S: Scheduler<T, Meta = R::ChildMeta>,
    {
        self.add_child_with(child, ())
    }
}

impl<T, M> RuntimeRootMut<'_, RoundRobin<T, M>, T>
where
    T: Send + 'static,
    M: WorkMeta,
{
    /// Remove a child by id.
    pub fn remove_child(&mut self, id: u32) -> Option<RemovedChild<T, M>> {
        self.root.remove_child(id)
    }

    /// Number of live children.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.root.len()
    }

    /// Whether the root currently has no live children.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.root.is_empty()
    }
}

impl<T, C> RuntimeRootMut<'_, StrictPriority<T, C>, T>
where
    T: Send + 'static,
    C: Scheduler<T>,
    C::Meta: WorkMeta,
{
    /// Register and attach one child scheduler at `priority`.
    pub fn add_priority_child(&mut self, child: C, priority: u8) -> (u8, u32) {
        self.add_child_with(child, priority)
    }

    /// Number of populated priority classes.
    #[must_use]
    pub fn class_count(&self) -> usize {
        self.root.class_count()
    }
}

impl<Ev, Err, M: WorkMeta> Clone for RuntimeHandle<Ev, Err, M> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            signals: self.signals.clone(),
            stats: self.stats.clone(),
        }
    }
}

impl<Ev, Err, M> RuntimeHandle<Ev, Err, M>
where
    Ev: Send + 'static,
    Err: Send + 'static,
    M: WorkMeta,
{
    /// Begin graceful shutdown. Idempotent. In-flight items still complete,
    /// queued outputs still drain, then [`Runtime::next`] emits a sticky
    /// shutdown.
    ///
    /// Wakes a driver parked inside [`Runtime::next`]; a driver sleeping
    /// on a [`RuntimeOutput::SleepUntil`] hint observes the shutdown at
    /// its next `next()` call.
    ///
    /// # Panics
    ///
    /// Can panic if the runtime mutex is poisoned, which only happens if
    /// a closure passed to [`Self::with_root_mut`] panicked.
    pub fn graceful_shutdown(&self) {
        let mut guard = self
            .shared
            .lock()
            .expect("dispatcher runtime mutex is poisoned");
        guard.shutdown = true;
        drop(guard);
        self.signals.wake();
    }

    /// Snapshot of runtime statistics. Lock-free; values are relaxed
    /// snapshots and may trail the driver by one step.
    #[must_use]
    pub fn stats(&self) -> RuntimeStats {
        RuntimeStats {
            in_flight: self.stats.in_flight.load(Ordering::Relaxed),
            dispatched: self.stats.dispatched.load(Ordering::Relaxed),
            output_queue: OutputQueueStats {
                queued: self.stats.output_queued.load(Ordering::Relaxed),
                capacity: None,
                dropped: 0,
            },
        }
    }

    /// Run `f` with shared access to the root downcast to `S`. `None` if
    /// the downcast fails.
    ///
    /// Holds the root lock for the duration of `f`; keep it short and do
    /// not re-enter the runtime from inside (it will deadlock).
    ///
    /// # Panics
    ///
    /// Can panic if the runtime mutex is poisoned, which only happens if
    /// a closure passed to [`Self::with_root_mut`] panicked.
    #[allow(clippy::unwrap_in_result)]
    pub fn with_root<S, R>(&self, f: impl FnOnce(&S) -> R) -> Option<R>
    where
        S: 'static,
    {
        let guard = self
            .shared
            .lock()
            .expect("dispatcher runtime mutex is poisoned");
        guard.root.as_any().downcast_ref::<S>().map(f)
    }

    /// Run `f` with exclusive access to the root downcast to `S`. `None`
    /// if the downcast fails.
    ///
    /// Holds the root lock for the duration of `f`; keep it short and do
    /// not re-enter the runtime from inside (it will deadlock).
    ///
    /// The closure receives a [`RuntimeRootMut`] rather than `&mut S`, so
    /// newly attached scheduler subtrees are registered automatically.
    ///
    /// # Panics
    ///
    /// Can panic if runtime mutex is poisoned. Which only can happen
    /// if any f passed to this function paniced.
    #[allow(clippy::unwrap_in_result)]
    pub fn with_root_mut<S, R>(
        &self,
        f: impl FnOnce(RuntimeRootMut<'_, S, FutureWork<Ev, Err>>) -> R,
    ) -> Option<R>
    where
        S: 'static,
    {
        let mutation = RuntimeMutationContext {
            queue_event_sink: self.signals.queue_event_sink(),
            _payload: PhantomData,
        };
        let mut guard = self
            .shared
            .lock()
            .expect("dispatcher runtime mutex is poisoned");
        let result = guard
            .root
            .as_any_mut()
            .downcast_mut::<S>()
            .map(|root| f(RuntimeRootMut { root, mutation }));
        drop(guard);
        self.signals.wake();
        result
    }
}

/// Single ordered output emitted by the runtime.
///
/// `R` defaults to [`crate::RuntimeEventType`], which is
/// [`core::convert::Infallible`] when the `runtime-events` feature is off
/// — `RuntimeOutput::Runtime(_)` is then unconstructible.
pub enum RuntimeOutput<Ev, Err, R = RuntimeEventType> {
    /// Terminal value of one work payload, plus its wall-clock latency.
    Work {
        /// `Ok(events)` (one or more events in order) or `Err(error)`.
        result: Result<Vec<Ev>, Err>,
        /// Latency between dispatch and completion.
        latency: Duration,
    },
    /// Out-of-band runtime event (only when `runtime-events` is enabled).
    Runtime(R),

    /// Scheduler readiness deadline. It may be emitted while payloads
    /// remain in flight. Drivers should retain the deadline and race
    /// sleeping until it against another call to [`Runtime::next`];
    /// calling `next()` earlier is always safe.
    SleepUntil(Instant),
    /// Sticky terminal output after graceful shutdown drains. Subsequent
    /// `next()` calls return this immediately.
    Shutdown,
}

enum RuntimeClock {
    Wallclock,
    Virtual { now: Instant, increment: Duration },
    Manual(ManualClock),
}

impl RuntimeClock {
    fn now(&mut self) -> Instant {
        match self {
            Self::Wallclock => Instant::now(),
            Self::Virtual { now, increment } => {
                *now += *increment;
                *now
            }
            Self::Manual(clock) => clock.now(),
        }
    }
}

/// Runtime-wide counters shared lock-free between the driver and
/// [`RuntimeHandle::stats`].
#[derive(Default)]
struct StatsCells {
    dispatched: AtomicU64,
    in_flight: AtomicU64,
    output_queued: AtomicUsize,
}

struct RuntimeSignals {
    state: Mutex<RuntimeSignalState>,
    next_queue_id: AtomicU64,
    queue_identity: Arc<()>,
}

#[derive(Default)]
struct RuntimeSignalState {
    waker: Option<Waker>,
    #[cfg(feature = "runtime-events")]
    events: VecDeque<crate::RuntimeEvent>,
}

impl Default for RuntimeSignals {
    fn default() -> Self {
        Self {
            state: Mutex::new(RuntimeSignalState::default()),
            next_queue_id: AtomicU64::new(0),
            queue_identity: Arc::new(()),
        }
    }
}

impl RuntimeSignals {
    fn register_waker(&self, waker: &Waker) {
        let mut state = self
            .state
            .lock()
            .expect("dispatcher runtime signal mutex is poisoned");
        if state.waker.as_ref().is_none_or(|old| !old.will_wake(waker)) {
            state.waker = Some(waker.clone());
        }
    }

    fn wake(&self) {
        let waker = self
            .state
            .lock()
            .expect("dispatcher runtime signal mutex is poisoned")
            .waker
            .take();
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    fn queue_event_sink(self: &Arc<Self>) -> crate::QueueEventSink {
        let event_signals = self.clone();
        let allocator_signals = self.clone();
        crate::QueueEventSink::new(
            move |event| event_signals.handle_queue_event(event),
            move || allocator_signals.allocate_queue_id(),
            self.queue_identity.clone(),
        )
    }

    fn allocate_queue_id(&self) -> crate::QueueId {
        let id = self
            .next_queue_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .expect("a runtime supports at most u64::MAX attached queues");
        crate::QueueId::new(id)
    }

    fn handle_queue_event(&self, event: crate::QueueEvent) {
        let mut state = self
            .state
            .lock()
            .expect("dispatcher runtime signal mutex is poisoned");
        #[cfg(feature = "runtime-events")]
        if let crate::QueueEvent::Drained { queue_id } = event {
            state
                .events
                .push_back(crate::RuntimeEvent::QueueDrained { queue_id });
        }
        #[cfg(not(feature = "runtime-events"))]
        let _ = event;
        let waker = state.waker.take();
        drop(state);
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    #[cfg(feature = "runtime-events")]
    fn take_events(&self) -> VecDeque<crate::RuntimeEvent> {
        let mut state = self
            .state
            .lock()
            .expect("dispatcher runtime signal mutex is poisoned");
        mem::take(&mut state.events)
    }
}

struct Shared<Ev, Err, M> {
    root: Box<dyn SchedulerObj<FutureWork<Ev, Err>, M>>,
    shutdown: bool,
    _phantom: PhantomData<(Ev, Err)>,
}

impl<Ev, Err, M> Shared<Ev, Err, M>
where
    Ev: 'static,
    Err: 'static,
    M: Send + 'static,
{
    fn next(&mut self, now: Instant) -> SharedNextResult<Ev, Err, M> {
        let r = self.root.update_ready(now);
        if r.ready {
            if let Some(work) = self.root.take_next() {
                return SharedNextResult::Work(work);
            }
        }
        r.next_update_at
            .map_or(SharedNextResult::Nothing, SharedNextResult::SleepUntil)
    }
}

enum SharedNextResult<Ev, Err, M>
where
    Ev: 'static,
    Err: 'static,
    M: Send + 'static,
{
    Work(ScheduledWork<FutureWork<Ev, Err>, M>),
    SleepUntil(Instant),
    Nothing,
}

struct InFlight<Ev, Err, M: WorkMeta> {
    start: Instant,
    work: Option<ScheduledWork<FutureWork<Ev, Err>, M>>,
}

impl<Ev, Err, M: WorkMeta> Unpin for InFlight<Ev, Err, M> {}

impl<Ev, Err, M: WorkMeta> Future for InFlight<Ev, Err, M> {
    type Output = CompletedWork<Ev, Err, M>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        let mut work = this
            .work
            .take()
            .expect("in-flight work polled after completion");
        match work.payload.as_mut().poll(cx) {
            Poll::Pending => {
                this.work = Some(work);
                Poll::Pending
            }
            Poll::Ready(result) => Poll::Ready(CompletedWork {
                start: this.start,
                result,
                meta: work.meta,
                routing: work.routing,
            }),
        }
    }
}

struct CompletedWork<Ev, Err, M> {
    start: Instant,
    meta: M,
    result: Result<Vec<Ev>, Err>,
    routing: RoutingPath,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use core::time::Duration;
    use std::{num::NonZeroUsize, time::Instant};

    use futures_util::future::{pending, poll_immediate};

    use super::{ClockConfig, FutureWork, ManualClock, Runtime, RuntimeConfig, RuntimeOutput};
    use crate::schedulers::{PeriodicLeaf, RoundRobin};

    type TestWork = FutureWork<u64, String>;
    type TestRoot = RoundRobin<TestWork, ()>;

    fn config() -> RuntimeConfig {
        RuntimeConfig {
            global_max_in_flight: NonZeroUsize::new(2).expect("non-zero"),
            clock: ClockConfig::Wallclock,
        }
    }

    fn firing_root() -> TestRoot {
        let mut root = TestRoot::new();
        root.add_child(PeriodicLeaf::new(Instant::now(), Duration::ZERO, || {
            Box::pin(async { Ok(vec![7_u64]) }) as TestWork
        }));
        root
    }

    #[tokio::test]
    async fn graceful_shutdown_drains_then_is_sticky() {
        let mut rt: Runtime<u64, String, ()> = Runtime::new(config(), firing_root());
        let handle = rt.handle();

        let mut works = 0_u64;
        while works < 3 {
            if let RuntimeOutput::Work { result, .. } = rt.next().await {
                assert_eq!(result.expect("payload succeeds"), vec![7]);
                works += 1;
            }
        }

        handle.graceful_shutdown();
        // Anything already in flight still drains as Work outputs, then
        // the terminal Shutdown arrives.
        loop {
            match rt.next().await {
                RuntimeOutput::Work { .. } => works += 1,
                RuntimeOutput::Shutdown => break,
                RuntimeOutput::SleepUntil(_) | RuntimeOutput::Runtime(_) => {}
            }
        }
        // Sticky: every subsequent call returns Shutdown immediately.
        assert!(matches!(rt.next().await, RuntimeOutput::Shutdown));
        assert!(matches!(rt.next().await, RuntimeOutput::Shutdown));

        let stats = handle.stats();
        assert_eq!(stats.dispatched, works, "every dispatch was drained");
        assert_eq!(stats.in_flight, 0);
        assert_eq!(stats.output_queue.queued, 0);
    }

    #[tokio::test]
    async fn graceful_shutdown_wakes_a_parked_driver() {
        // An empty root reports not-ready with no hint: the driver parks.
        let mut rt: Runtime<u64, String, ()> = Runtime::new(config(), TestRoot::new());
        let handle = rt.handle();

        let mut fut = rt.next();
        assert!(
            poll_immediate(&mut fut).await.is_none(),
            "nothing to do: the driver must park"
        );
        handle.graceful_shutdown();
        assert!(matches!(fut.await, RuntimeOutput::Shutdown));
    }

    #[tokio::test]
    async fn stats_count_dispatches() {
        let mut rt: Runtime<u64, String, ()> = Runtime::new(config(), firing_root());
        let handle = rt.handle();
        assert_eq!(handle.stats().dispatched, 0);

        let mut works = 0_u64;
        while works < 5 {
            if let RuntimeOutput::Work { .. } = rt.next().await {
                works += 1;
            }
        }
        assert!(handle.stats().dispatched >= 5);
    }

    #[tokio::test]
    async fn deadline_dispatches_due_work_while_other_work_is_in_flight() {
        let clock = ManualClock::new();
        let now = clock.now();
        let deadline = now + Duration::from_secs(1);
        let interval = Duration::from_mins(1);

        let mut root = TestRoot::new();

        root.add_child(PeriodicLeaf::new(now, interval, || {
            Box::pin(pending()) as TestWork
        }));

        root.add_child(PeriodicLeaf::starting_at(now, deadline, interval, || {
            Box::pin(async { Ok(vec![9_u64]) }) as TestWork
        }));

        let mut rt = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: NonZeroUsize::new(2).expect("non-zero"),
                clock: ClockConfig::Manual(clock.clone()),
            },
            root,
        );

        let handle = rt.handle();

        let output = poll_immediate(rt.next()).await;

        assert!(matches!(
            output,
            Some(RuntimeOutput::SleepUntil(observed)) if observed == deadline
        ));

        assert_eq!(handle.stats().in_flight, 1);

        let output = poll_immediate(rt.next()).await;

        assert!(output.is_none());

        clock.advance_to(deadline);

        let output = rt.next().await;

        assert!(matches!(
            output,
            RuntimeOutput::Work { result: Ok(events), .. } if events == vec![9]
        ));
    }

    #[tokio::test]
    async fn shutdown_does_not_emit_a_sleep_hint_while_work_is_in_flight() {
        let clock = ManualClock::new();
        let now = clock.now();
        let deadline = now + Duration::from_secs(1);
        let interval = Duration::from_mins(1);
        let mut root = TestRoot::new();

        root.add_child(PeriodicLeaf::new(now, interval, || {
            Box::pin(async { Ok(vec![9_u64]) }) as TestWork
        }));

        root.add_child(PeriodicLeaf::new(now, interval, || {
            Box::pin(pending()) as TestWork
        }));

        root.add_child(PeriodicLeaf::starting_at(now, deadline, interval, || {
            Box::pin(async { Ok(vec![10_u64]) }) as TestWork
        }));

        let mut rt = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: NonZeroUsize::new(3).expect("non-zero"),
                clock: ClockConfig::Manual(clock),
            },
            root,
        );

        let handle = rt.handle();

        assert!(matches!(rt.next().await, RuntimeOutput::Work { .. }));

        handle.graceful_shutdown();

        assert!(poll_immediate(rt.next()).await.is_none());
    }

    #[tokio::test]
    async fn with_root_reads_and_with_root_mut_mutates() {
        let rt: Runtime<u64, String, ()> = Runtime::new(config(), firing_root());
        let handle = rt.handle();

        assert_eq!(handle.with_root::<TestRoot, _>(TestRoot::len), Some(1));
        assert_eq!(handle.with_root::<u32, _>(|_| ()), None, "wrong type");

        let id = handle
            .with_root_mut::<TestRoot, _>(|mut root| {
                root.add_child(PeriodicLeaf::new(
                    Instant::now(),
                    Duration::from_secs(9999),
                    || Box::pin(async { Ok(vec![9_u64]) }) as TestWork,
                ))
            })
            .expect("root downcasts");
        assert_eq!(handle.with_root::<TestRoot, _>(TestRoot::len), Some(2));

        handle
            .with_root_mut::<TestRoot, _>(|mut root| {
                root.remove_child(id).expect("child exists");
            })
            .expect("root downcasts");
        assert_eq!(handle.with_root::<TestRoot, _>(TestRoot::len), Some(1));
    }
}
