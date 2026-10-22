<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# nv-redfish-bmc-mock

Test Redfish client workflows by scripting the BMC operations they should
perform. Queue expected requests and JSON replies, run your client code, and
check the result. The mock validates operation order, resource URIs, and request
payloads in memory.

`Bmc<E>` implements the transport interface from
[`nv-redfish-core`](https://crates.io/crates/nv-redfish-core), so the same
generated models and service wrappers can use it. `E` is the error type you can
inject into scripted responses; use `Infallible` for success-only scenarios.

## Script a read

Add these dependencies to your test crate:

```toml
[dev-dependencies]
nv-redfish-bmc-mock = "0.1"
nv-redfish-core = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["macros", "rt"] }
```

```rust
use nv_redfish_bmc_mock::{Bmc, Expect};
use nv_redfish_core::{Bmc as _, ODataId, Raw};
use serde_json::json;
use std::convert::Infallible;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bmc = Bmc::<Infallible>::default();
    bmc.expect(Expect::get(
        "/redfish/v1",
        json!({
            "@odata.id": "/redfish/v1",
            "RedfishVersion": "1.19.0"
        }),
    ));

    let root = bmc.get::<Raw>(&ODataId::service_root()).await?;
    assert_eq!(root.value()["RedfishVersion"], "1.19.0");
    Ok(())
}
```

Use a generated entity instead of `Raw` to test typed deserialization. For
high-level workflows, pass `Arc<Bmc<E>>` to the service wrappers in
[`nv-redfish`](https://crates.io/crates/nv-redfish).

## Expect writes and task completion

Choose helpers for the operation and outcome your code must handle:

| Helpers | Scenario |
| --- | --- |
| `get`, `expand` | Read a resource or expanded representation. |
| `update`, `update_with_etag` | Match an update payload and optionally its `ETag`. |
| `create`, `delete`, `action` | Create a resource, delete one, or invoke an action. |
| `update_task`, `create_task`, `delete_task`, `action_task` | Return an asynchronous task handle. |
| `update_empty`, `create_empty`, `action_empty` | Complete without a response body. |
| `update_status`, `action_status`, `poll_status` | Return an HTTP status error. |
| `poll_pending`, `poll_result`, `poll_empty`, `poll_wait` | Script task progress, completion, or a poll that stays pending. |
| `create_session` | Return a session entity, token, and logout URI. |
| `multipart_update`, `multipart_update_with_oem_parts` | Match an upload's parameters, file name, and OEM part names. |
| `stream`, `stream_events` | Supply event payloads and optional event IDs. |

For example, verify an update's URI and JSON body, then inspect the returned
entity:

```rust
use nv_redfish_bmc_mock::{Bmc, Expect};
use nv_redfish_core::{Bmc as _, ModificationResponse, ODataId, Raw};
use serde_json::json;
use std::convert::Infallible;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bmc = Bmc::<Infallible>::default();
    let id = ODataId::from("/redfish/v1/Chassis/1".to_owned());
    let update = json!({"AssetTag": "rack-7"});
    bmc.expect(Expect::update(
        &id,
        &update,
        json!({"@odata.id": id.to_string(), "AssetTag": "rack-7"}),
    ));

    let response = bmc.update::<_, Raw>(&id, None, &update).await?;
    let ModificationResponse::Entity(chassis) = response else {
        panic!("expected the updated entity");
    };
    assert_eq!(chassis.value()["AssetTag"], "rack-7");
    Ok(())
}
```

JSON arguments accept values or JSON text through `Display`. Helpers parse them
when the expectation is created. Request bodies are compared as JSON values,
so object key order and whitespace do not affect matching.

## Matching and errors

Expectations form one FIFO queue. Each scripted operation consumes the next
expectation; a different operation, URI, or checked payload returns an
`Unexpected...` error. An operation after the queue is exhausted returns
`Error::NothingIsExpected`. Concurrent callers must preserve the scripted
request order.

To inject a custom failure, construct `Expect { request, response: Err(error) }`
using an `ExpectedRequest` variant. The mock returns it as
`Error::ErrorResponse`. `debug_expect()` prints expectations still in the queue.
Tests must arrange their own checks that the workflow reached its final step;
dropping the mock does not assert that every expectation was used.

Expansion expectations match the resource URI and supply the expanded JSON;
they do not validate the `$expand` expression. Multipart expectations check
parameters, file name, and OEM part names; they do not read upload bytes.
`Bmc::filter` and `filter_repaired` are currently unimplemented and panic.

Repaired reads use the same GET or expand expectations, then apply the supplied
`Repair` before typed decoding. `stream_events` expectations match the requested
last event ID and provide the IDs and payloads the consumer should see.

## License

Apache-2.0.
