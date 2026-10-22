<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# nv-redfish-server-core

Shared types for building Redfish JSON responses in servers and simulators.
They represent resource links, finite numbers, UUIDs, and extension properties
used by generated response structs.

Generate models with
[`nv-redfish-csdl-compiler`](https://crates.io/crates/nv-redfish-csdl-compiler),
populate them with your resource data, and serialize them with Serde in your
HTTP handlers. You can also use these types in handwritten response structs.

## Installation

```toml
[dependencies]
nv-redfish-server-core = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## Shared types

| Type | Purpose |
| --- | --- |
| `Reference` | A resource link serialized as `{"@odata.id": "..."}`. |
| `FiniteF64` | A JSON number that rejects `NaN` and infinity at construction. |
| `NonFiniteNumber` | The error returned when converting a non-finite `f64`. |
| `EdmGuid` | A re-export of `uuid::Uuid`, serialized as a UUID string. |
| `AdditionalProperties` | A `BTreeMap<String, JsonValue>` for extension fields. |
| `JsonValue` | A re-export of `serde_json::Value`. |
| `Enum` | A trait implemented by generated enums to expose their exact CSDL wire spelling. |

## Serialize response fields

Embed the shared types in a Serde response struct:

```rust
use nv_redfish_server_core::{FiniteF64, Reference};
use serde::Serialize;
use std::convert::TryFrom;

#[derive(Serialize)]
struct SensorReading {
    #[serde(rename = "Reading")]
    reading: FiniteF64,
    #[serde(rename = "RelatedItem")]
    related_item: Vec<Reference>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let response = SensorReading {
        reading: FiniteF64::try_from(34.25)?,
        related_item: vec![Reference::new("/redfish/v1/Chassis/1")],
    };

    println!("{}", serde_json::to_string_pretty(&response)?);
    Ok(())
}
```

`Reference::new` accepts a URI string; `From<&str>` and `From<String>` also
allow `.into()`. It serializes the link with its `@odata.id` key.

Convert measured values with `FiniteF64::try_from(value)` and handle
`NonFiniteNumber` if the value is `NaN` or infinite. `get()` returns the stored
`f64`.

Flatten `AdditionalProperties` only where the schema allows extension fields.
Its entries serialize alongside the struct's known properties; the application
supplies their names and values.

## Use generated response models

The compiler's `generator::server_rust` API generates serializable structs
using these primitives. Constructors take required properties; setters add
optional fields. Resource links use `Reference`, while excerpt properties use
separate generated structs containing the selected fields.

See the
[server response example](https://github.com/dsx-ai-factory/nv-redfish/tree/main/examples/server-response)
for a build script and responses containing a service root, chassis, sensors,
and thermal metrics. From the repository, run:

```sh
cargo run -p server-response
```

Your application fills resource values, constructs URIs, routes requests, and
serializes the response models through its chosen HTTP framework.

## License

Apache-2.0.
