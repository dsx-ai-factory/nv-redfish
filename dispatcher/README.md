<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# nv-redfish-dispatcher

Schedule asynchronous work within device and service limits.

A monitoring service may poll hundreds of BMCs at different intervals.
It needs to limit requests per device and across the fleet, prioritize urgent
work, and pause requests to failing devices.

`nv-redfish-dispatcher` decides which operation may start next. You supply
futures and compose policies into a tree: per-device concurrency limits and
circuit breakers, round-robin selection across devices, and a global concurrency
cap. Periodic tasks and queued jobs share these policies.

The dispatcher drives admitted futures, returns their results, and updates
policy state when work finishes. Your application provides the operations and
async executor. The same policies work for Redfish requests and other
asynchronous jobs.

## Run periodic work

Add the dispatcher and an async executor. This example uses Tokio:

```toml
[dependencies]
nv-redfish-dispatcher = "0.1"
tokio = { version = "1", features = ["macros", "rt", "time"] }
```

Create two periodic sources, limit each to one operation in flight, and select
between them with round-robin scheduling:

```no_run
use nv_redfish_dispatcher::{
    BoundedConcurrency, ClockConfig, FutureWork, PeriodicLeaf, RoundRobin,
    Runtime, RuntimeConfig, RuntimeOutput,
};
use std::convert::Infallible;
use std::num::{NonZeroU32, NonZeroUsize};
use std::time::{Duration, Instant};

type Work = FutureWork<&'static str, Infallible>;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut root = RoundRobin::<Work, ()>::new();
    for source in ["sensor-a", "sensor-b"] {
        let leaf = PeriodicLeaf::new(
            Instant::now(),
            Duration::from_secs(1),
            move || -> Work { Box::pin(async move { Ok(vec![source]) }) },
        );
        root.add_child(BoundedConcurrency::new(
            NonZeroU32::new(1).expect("non-zero cap"),
            leaf,
        ));
    }

    let mut runtime = Runtime::new(
        RuntimeConfig {
            global_max_in_flight: NonZeroUsize::new(8).expect("non-zero cap"),
            clock: ClockConfig::Wallclock,
        },
        root,
    );
    let handle = runtime.handle();
    let mut deadline = None;
    let mut completed = 0;

    loop {
        let output = if let Some(at) = deadline {
            tokio::select! {
                output = runtime.next() => output,
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(at)) => {
                    deadline = None;
                    continue;
                }
            }
        } else {
            runtime.next().await
        };

        match output {
            RuntimeOutput::Work { result, .. } => {
                println!("{:?}", result.expect("infallible work"));
                completed += 1;
                if completed >= 4 {
                    handle.graceful_shutdown();
                }
            }
            RuntimeOutput::SleepUntil(at) => deadline = Some(at),
            RuntimeOutput::Runtime(_) => {}
            RuntimeOutput::Shutdown => break,
        }
    }
}
```

`PeriodicLeaf::new` starts immediately. Subsequent ticks are scheduled from
dispatch time; work held back by a policy resumes without a catch-up burst.
`BoundedConcurrency` with a cap of one prevents overlapping runs of a source.
Use `PeriodicLeaf::starting_at` to stagger the first dispatch across sources.

## Drive the runtime

Keep calling `Runtime::next` to dispatch work, poll futures, and receive outputs.
The application consumes each `RuntimeOutput`:

| Output | Action |
| --- | --- |
| `Work { result, latency }` | Handle the completed payload's events or error. |
| `SleepUntil(Instant)` | Retain the deadline and race the timer against the next call to `next`. |
| `Runtime(event)` | Handle optional runtime events. |
| `Shutdown` | Exit the driver loop after graceful shutdown. |

A sleep deadline can arrive while other payloads are still running. Racing the
timer against `next`, as above, lets completions, newly queued work, and control
changes wake the driver promptly. The runtime relies on the application's
executor for polling and on its timer for wall-clock deadlines.

`RuntimeConfig::global_max_in_flight` caps dispatched work across the whole tree.
`RuntimeHandle::graceful_shutdown` stops new dispatches, lets in-flight work
finish, and drains queued outputs before returning `Shutdown`.

For deterministic tests, `ClockConfig::Manual` uses a shared `ManualClock`.
Construct time-dependent schedulers with `clock.now()`, advance the clock to
the requested deadline, and call `next` again.

## Compose scheduling policies

Built-in nodes are available from the crate root and the `schedulers` module:

| Component | Role |
| --- | --- |
| `PeriodicLeaf` | Produce work from a factory at a configured interval. |
| `BoundedQueue` | Accept externally submitted work through cloneable producers. |
| `RoundRobin` | Rotate across ready child schedulers sharing a metadata type. |
| `StrictPriority` | Prefer higher numeric priority classes; rotate within each class. |
| `BoundedConcurrency` | Cap in-flight work in one subtree. |
| `TokenBucket` | Limit admission by work cost, refill rate, and burst capacity. |
| `FixedCost` | Add a constant cost to a child's metadata for cost-aware policies. |
| `CircuitBreaker` | Pause a failing subtree, then admit a recovery probe after a cooldown. |

For example, wrap a periodic leaf in `BoundedConcurrency`, add its request cost
with `FixedCost`, then place it under `TokenBucket`. Put several such subtrees
under `RoundRobin` to share dispatch opportunities across sources.

`ScheduledWork<T, M>` keeps the payload, metadata, and completion routing path
together. `WorkMeta` accepts any `Send + 'static` type; `()` is suitable when
no policy metadata is needed. `WithCost` and `WithPriority` add annotations,
exposed through `HasCost` and `HasPriority`, while preserving wrapped metadata.

### Completion timing

`Completion::completed_at` records when the driver receives a job's result.
The runtime reads its configured clock after the job's future returns the
result. Reading the completion time does not advance the virtual clock.

When `CircuitBreaker` opens, it sets the recovery deadline to
`completed_at + cool_down`. A failed recovery probe starts another cooldown.
Time spent running the job does not count toward that pause. The probe also
waits for earlier jobs to finish and for the child scheduler to have work ready.

If you drive a scheduler without `Runtime`, use the same clock for
`completed_at` and `Scheduler::update_ready`. Wrappers must pass the timestamp
unchanged to their child scheduler, even when removing their own metadata.

Code constructing a `Completion` must set `completed_at`. Patterns that list
every field must include `completed_at` or use `..` to ignore unlisted fields.

## Submit queued work

Build a bounded queue, attach its scheduler leaf to the tree, and retain the
producer for submitting work from elsewhere in the application:

```rust
use nv_redfish_dispatcher::{
    BoundedQueueBuilder, EnqueueOutcome, FutureWork, RoundRobin, ScheduledWork,
};
use std::convert::Infallible;
use std::num::NonZeroUsize;

type Work = FutureWork<&'static str, Infallible>;

let (queue, producer) = BoundedQueueBuilder::new(
    NonZeroUsize::new(128).expect("non-zero capacity"),
)
.fifo()
.build::<Work, ()>();

let mut root = RoundRobin::<Work, ()>::new();
root.add_child(queue);

let work: Work = Box::pin(async { Ok(vec!["queued task completed"]) });
let outcome = producer.try_push(ScheduledWork::new((), work));
assert!(matches!(outcome, EnqueueOutcome::Admitted));
```

Use this root when constructing `Runtime`. A producer wakes the attached runtime
when an empty queue becomes ready. `try_push` returns immediately:

- `Admitted`: the queue accepted the work.
- `Rejected(work)`: admission rejected it; ownership returns to the caller.
- `Evicted { work }`: the incoming item was accepted and an existing queued
  item returned.
- `Closed(work)`: the queue is closed; ownership returns to the caller.

The default `TailDrop` policy rejects incoming items when the queue is full.
Use `admission_policy` for a custom `AdmissionPolicy`. Select dequeue order with
`fifo()` or `discipline(...)`; `StochasticFairQueue` distributes metadata-defined
flows across a fixed number of FIFO buckets and rotates among non-empty buckets.

`producer.close()` stops admission and allows accepted work to drain while the
driver keeps running. The lifecycle is `Draining` while queued or in-flight
work remains, then `Drained`. Wait for `Drained` before removing a queue from its
parent. To finish all accepted work before runtime shutdown, close the producers,
drive until their queues drain, then request `graceful_shutdown`.

## Control and observe a running tree

`RuntimeHandle` is cloneable. Use `with_root` to inspect the root and
`with_root_mut` to mutate it, specifying its concrete type for the downcast.
Mutation uses `RuntimeRootMut`, which registers newly attached subtrees with
the runtime. The built-in branch helpers support adding children to
`RoundRobin` and priority children to `StrictPriority`.

`handle.stats()` reports total dispatched work, current in-flight work, and
queued outputs. Queue producers expose their own `stats()`, including depth,
capacity, admission rejections, evictions, and lifecycle.

Enable `runtime-events` to receive dispatch, completion, throttle, and queue-drain
notifications through `RuntimeOutput::Runtime`. They share the ordered output
interface with work results.

## Implement a scheduler

Implement `Scheduler<T>` to introduce a new leaf or policy:

1. `update_ready(now)` reports readiness, an optional next deadline, and an
   optional cost hint through `Readiness`.
2. `take_next()` returns a `ScheduledWork` when the node can dispatch an item.
3. `on_complete(completion)` receives the outcome and latency of dispatched work.
4. `register_queue_event_sink(sink)` retains the sink for externally fed leaves
   or forwards it to child schedulers.

Branches stamp `RoutingPath` as work travels toward the root and consume their
routing entries when forwarding completions. Metadata wrappers added on
dispatch must be removed before a completion returns to the child.

The
[composable tree example](https://github.com/dsx-ai-factory/nv-redfish/blob/main/dispatcher/examples/composable_tree.rs)
implements custom nodes, attaches subtrees at runtime, and drives shutdown.
Run it from the repository with:

```sh
cargo run -p nv-redfish-dispatcher --example composable_tree
```

## License

Apache-2.0.
