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

#![doc = include_str!("../README.md")]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::suspicious,
    clippy::complexity,
    clippy::perf
)]
#![deny(
    clippy::absolute_paths,
    clippy::todo,
    clippy::tests_outside_test_module,
    clippy::panic,
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    clippy::unused_trait_names,
    clippy::print_stdout,
    clippy::print_stderr
)]
#![deny(missing_docs)]
#![allow(clippy::doc_markdown)]
// Module-name repetition is intentional for this crate's public types
// (RuntimeOutput, RuntimeEvent, RuntimeStats, etc.) which are re-exported.
#![allow(clippy::module_name_repetitions)]

pub mod event;
pub mod runtime;
pub mod scheduler;
pub mod schedulers;
pub mod stats;
pub mod work;

#[cfg(feature = "runtime-events")]
#[doc(inline)]
pub use event::RuntimeEvent;
#[doc(inline)]
pub use event::RuntimeEventType;
#[doc(inline)]
pub use event::{QueueEvent, QueueEventSink, QueueId};
#[doc(inline)]
pub use runtime::ClockConfig;
#[doc(inline)]
pub use runtime::FutureWork;
#[doc(inline)]
pub use runtime::ManualClock;
#[doc(inline)]
pub use runtime::Runtime;
#[doc(inline)]
pub use runtime::RuntimeConfig;
#[doc(inline)]
pub use runtime::RuntimeHandle;
#[doc(inline)]
pub use runtime::RuntimeOutput;
#[doc(inline)]
pub use runtime::RuntimeRootMut;
#[doc(inline)]
pub use scheduler::RuntimeChildContainer;
#[doc(inline)]
pub use scheduler::ScheduledWork;
#[doc(inline)]
pub use scheduler::Scheduler;
#[doc(inline)]
pub use stats::OutputQueueStats;
#[doc(inline)]
pub use stats::RuntimeStats;
#[doc(inline)]
pub use work::Completion;
#[doc(inline)]
pub use work::CompletionOutcome;
#[doc(inline)]
pub use work::CostUnits;
#[doc(inline)]
pub use work::HasCost;
#[doc(inline)]
pub use work::HasPriority;
#[doc(inline)]
pub use work::Readiness;
#[doc(inline)]
pub use work::RoutingPath;
#[doc(inline)]
pub use work::WithCost;
#[doc(inline)]
pub use work::WithPriority;
#[doc(inline)]
pub use work::WorkMeta;

#[doc(inline)]
pub use schedulers::Fifo;
#[doc(inline)]
pub use schedulers::StochasticFairQueue;
#[doc(inline)]
pub use schedulers::{
    AdmissionContext, AdmissionDecision, AdmissionPolicy, BoundedQueue, BoundedQueueBuilder,
    BoundedQueuePair, BoundedQueueProducer, BoundedQueueStats, EnqueueOutcome, QueueDiscipline,
    QueueEntryId, QueueEntryRef, QueueLifecycle, TailDrop,
};
#[doc(inline)]
pub use schedulers::{
    BoundedConcurrency, BreakerState, CircuitBreaker, CircuitBreakerConfig, FixedCost,
    PeriodicLeaf, RemovedChild, RoundRobin, StrictPriority, TokenBucket, TokenBucketConfig,
};
