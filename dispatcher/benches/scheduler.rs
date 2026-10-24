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

//! Instruction-count benchmarks (gungraun / Valgrind Callgrind).

#[cfg(unix)]
mod unix {
    use core::mem;
    use core::sync::atomic::AtomicBool;
    use core::time::Duration;
    use std::hint::black_box;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::time::Instant;

    use gungraun::library_benchmark;
    use nv_redfish_dispatcher::{
        ClockConfig, Completion, CompletionOutcome, RemovedChild, RoundRobin, Runtime,
        RuntimeConfig, ScheduledWork, Scheduler,
    };

    use nv_redfish_dispatcher_sim::{ample_bucket, source, source_due_at, Err, Ev, Meta, Work};

    pub type Tree = RoundRobin<Work, Meta>;
    type Removed = Option<RemovedChild<Work, Meta>>;
    type BenchRuntime = Runtime<Ev, Err, Meta>;

    /// Fleet of `n` sources; when `sparse`, only the last one in
    /// rotation order has work due, so a dispatch scans the other n-1.
    fn fleet(n: u32, sparse: bool) -> (Tree, Instant) {
        let now = Instant::now();
        let no_fail = Arc::new(AtomicBool::new(false));
        let mut root = Tree::new();
        for idx in 0..n {
            let first_due = if sparse && idx != n - 1 {
                now + Duration::from_secs(3600)
            } else {
                now
            };
            root.add_child(source_due_at(
                now,
                first_due,
                idx,
                ample_bucket(),
                no_fail.clone(),
            ));
        }
        (root, now)
    }

    /// Fleet plus a pre-built extra source, so churn benchmarks measure
    /// only the mutation, not subtree construction.
    fn fleet_with_spare(n: u32) -> (Tree, Box<dyn Scheduler<Work, Meta = Meta>>) {
        let (root, now) = fleet(n, false);
        let no_fail = Arc::new(AtomicBool::new(false));
        (root, Box::new(source(now, n, ample_bucket(), no_fail)))
    }

    /// Fleet with one item already dispatched (and still in flight)
    /// from the first source in rotation.
    fn fleet_with_in_flight(n: u32) -> (Tree, ScheduledWork<Work, Meta>) {
        let (mut root, now) = fleet(n, false);
        black_box(root.update_ready(now));
        let work = root.take_next().expect("a due source dispatches");
        (root, work)
    }

    /// Fleet with half its sources removed while idle, leaving stale
    /// rotation entries for this dispatch to purge lazily.
    fn fleet_half_removed(n: u32) -> (Tree, Instant) {
        let (mut root, now) = fleet(n, false);
        for id in 0..n / 2 {
            black_box(root.remove_child(id));
        }
        (root, now)
    }

    /// Runtime with `n` existing sources and one pre-built child. Runtime
    /// construction and the initial sink registration are fixture setup;
    /// the benchmark measures only registering and attaching the new child.
    fn runtime_with_spare(n: u32) -> (BenchRuntime, Box<dyn Scheduler<Work, Meta = Meta>>) {
        let (root, now) = fleet(n, false);
        let no_fail = Arc::new(AtomicBool::new(false));
        let child = Box::new(source(now, n, ample_bucket(), no_fail));
        let runtime = Runtime::new(
            RuntimeConfig {
                global_max_in_flight: NonZeroUsize::MIN,
                clock: ClockConfig::Wallclock,
            },
            root,
        );
        (runtime, child)
    }

    fn dispatch((mut root, now): (Tree, Instant)) -> Tree {
        black_box(root.update_ready(now));
        if let Some(work) = root.take_next() {
            root.on_complete(Completion {
                completed_at: now,
                outcome: CompletionOutcome::Succeeded,
                latency: Duration::ZERO,
                meta: work.meta,
                routing: work.routing,
            });
        }
        root
    }

    #[library_benchmark]
    #[bench::one_source(fleet(1, false))]
    #[bench::dense_100(fleet(100, false))]
    #[bench::dense_1000(fleet(1000, false))]
    pub fn dense_dispatch(input: (Tree, Instant)) -> Tree {
        dispatch(input)
    }

    #[library_benchmark]
    #[bench::sparse_100(fleet(100, true))]
    #[bench::sparse_1000(fleet(1000, true))]
    pub fn sparse_dispatch(input: (Tree, Instant)) -> Tree {
        dispatch(input)
    }

    // Nothing in flight: removal detaches the subtree immediately.
    #[library_benchmark]
    #[bench::n_1000(fleet_with_spare(1000))]
    pub fn churn_detached(
        (mut root, child): (Tree, Box<dyn Scheduler<Work, Meta = Meta>>),
    ) -> (Tree, Removed) {
        let id = root.add_child(child);
        let removed = root.remove_child(id);
        (root, removed)
    }

    // An item is in flight: removal quarantines the subtree until its
    // completion drains.
    #[library_benchmark]
    #[bench::n_1000(fleet_with_in_flight(1000))]
    pub fn churn_draining(
        (mut root, work): (Tree, ScheduledWork<Work, Meta>),
    ) -> (Tree, Removed, ScheduledWork<Work, Meta>) {
        let removed = root.remove_child(0);
        (root, removed, work)
    }

    // Measures the deferred cost of O(1) removal: the take_next sweep
    // over stale queue entries.
    #[library_benchmark]
    #[bench::n_1000(fleet_half_removed(1000))]
    pub fn stale_purge_dispatch(input: (Tree, Instant)) -> Tree {
        dispatch(input)
    }

    // Registering one new subtree must be independent of the number of
    // schedulers already attached to the runtime.
    #[library_benchmark]
    #[bench::existing_1(runtime_with_spare(1))]
    #[bench::existing_100(runtime_with_spare(100))]
    #[bench::existing_1000(runtime_with_spare(1000))]
    pub fn dynamic_add_child(
        (runtime, child): (BenchRuntime, Box<dyn Scheduler<Work, Meta = Meta>>),
    ) {
        let handle = runtime.handle();
        let added = handle.with_root_mut::<Tree, _>(|mut root| root.add_child(child));
        black_box(added);

        // Dropping the fixture would walk the whole tree inside the
        // measurement and obscure the complexity of the mutation itself.
        mem::forget(runtime);
    }
}

#[cfg(unix)]
use unix::{
    churn_detached, churn_draining, dense_dispatch, dynamic_add_child, sparse_dispatch,
    stale_purge_dispatch,
};

#[cfg(unix)]
gungraun::library_benchmark_group!(
    name = scheduler;
    benchmarks =
        dense_dispatch,
        sparse_dispatch,
        churn_detached,
        churn_draining,
        stale_purge_dispatch,
        dynamic_add_child
);

#[cfg(unix)]
gungraun::main!(library_benchmark_groups = scheduler);

#[cfg(not(unix))]
fn main() {}
