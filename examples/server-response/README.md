<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
-->

# Generated Redfish server models

```sh
cargo run -p server-response
```

Prints a document containing `ServiceRoot`, `ChassisCollection`, one `Chassis`,
`SensorCollection`, two `Sensors`, `ThermalSubsystem`, and `ThermalMetrics`.
The outer document is an example container,
not a Redfish resource. The program serializes ordinary Rust structs directly
into stdout; it does not construct an intermediate JSON tree.

The responses form this chain of resource links:

```text
/redfish/v1
└── Chassis
    └── HGX_CPU_0
        ├── Sensors
        │   ├── ProcessorModule_0_CPU_0_TempAvg_0   34.25 Cel
        │   └── ProcessorModule_0_CPU_0_Power_0     56.315 W
        └── ThermalSubsystem
            └── ThermalMetrics
```

Chassis and sensor names, identifiers, readings, ranges, and status values come
from the Supermicro GB300 NVL72 capture in `redfish-test-vectors`:

```text
vectors/supermicro/gb300-nvl72/70-01-00-14-2026-06-05-307dcf085158/redfish/v1/
```

Only the selected chassis and sensors are included, with collection counts
adjusted accordingly. Thermal metrics reuse the selected temperature sensor's
reading and URI in two generated excerpt models:

- `TemperatureReadingsCelsius` contains a `SensorExcerptArray`, including the
  sensor's `Reading`, `DataSourceUri`, and `DeviceName`.
- `TemperatureSummaryCelsius.Internal` contains a `SensorExcerpt` with the same
  `Reading` and `DataSourceUri`.

These nested objects contain excerpt fields, without automatic `@odata.id`,
`@odata.type`, or collection count annotations. The full sensor responses remain
separate resources referenced by `DataSourceUri`.

`ServiceRoot.Links.Sessions` retains the required session
collection link; its target is outside this example. No external test-vector
checkout is needed to build or run it.

`build.rs` selects `ServiceRoot.ServiceRoot`, `ChassisCollection.ChassisCollection`,
`Chassis.Chassis`, `SensorCollection.SensorCollection`, `Sensor.Sensor`,
`ThermalSubsystem.ThermalSubsystem`, and `ThermalMetrics.ThermalMetrics` from
the bundled schemas and generates one `OUT_DIR/redfish_server.rs` file with inline
namespace modules. See `src/main.rs` for their use. Generated `@odata.type` values
follow these bundled schemas rather than the older versions in the capture.

The compiler result passes through the same optimizer as client generation:
inheritance chains are folded and namespaces shortened where unambiguous.
The example uses `chassis::Chassis`; its `ODATA_TYPE` retains the original CSDL version.

All server generator entry points accept `compiler::QualifiedName` roots.
`build.rs` parses textual names into owned `edmx::QualifiedTypeName` values
and borrows them as `QualifiedName` for generation.

Client and server generation share Rust names, identifier escaping, and schema
documentation through `generator::common_rust`. The server generator does not
depend on `client_rust`. It borrows property names and compiled schema properties;
`StructDef`, `EnumDef`, and `TypeDef` retain schema data until emission. Collected
properties refer to structural/navigation declarations or protocol annotations.
During struct emission, `PropertyDef` turns those sources into Rust field types,
documentation, serde attributes, and setters. Rust tokens are borrowed when
unchanged and owned when a wrapper is generated.
Property names remain typed CSDL references until Rust identifiers and serde
string literals are emitted. Protocol annotations have a separate representation;
collection counts retain a reference to their navigation property name. Namespace
segments likewise retain their typed `ModName` representation through emission.

## Constructors and required properties

The constructor rule mirrors the client Action argument threshold:

- At most three required fields: `new(first, second, third)`.
- More than three: `new(TypeRequired { ... })`.
- No required fields: `new()`.

The inclusive default threshold is 3. It can be changed through
`server_rust::Config::constructor_max_param_number_threshold`, passed to
`Builder::new(&roots).config(config)`, followed by `.generate(Input::Bundle(&bundle))`
or `.generate(Input::Files(&paths))`, where `paths` contains `PathBuf` values.

```rust
let chassis = Chassis::new(ChassisRequired {
    odata_id: "/redfish/v1/Chassis/HGX_CPU_0".into(),
    id: "HGX_CPU_0".into(),
    name: "HGX_CPU_0".into(),
    chassis_type: ChassisType::Component,
})
.status(Status::new().state(State::Enabled).health(Health::Ok));
```

Required fields include inherited `Redfish.Required` properties. Entity
constructors also require explicit `@odata.id`, which counts toward the threshold.
`@odata.type` is initialized from the generated `ODATA_TYPE` associated constant;
it is not a constructor argument and does not count toward the threshold. The
constant uses the fully qualified name of the resolved CSDL type, including its
version. Selecting the latest schema therefore selects its type string too.
`RequiredOnCreate` applies to request creation, not to response construction.

URI construction is deferred. A possible next step is to parse `Redfish.Uris`
templates and generate typed identifier arguments, including parent identifiers.
For now `@odata.id` remains an explicit string supplied by the application.

Neither a type with required fields nor its `TypeRequired` helper implements
`Default`. Omitting a mandatory argument or helper field fails compilation.
A type without required fields also implements `Default`.

## Field representation

| Property | Rust field | Serialization |
| --- | --- | --- |
| Required, non-nullable | `T` | Always present |
| Required, nullable | `Option<T>` | Always present, including null |
| Optional, non-nullable | `Option<T>` | None omits the property |
| Optional, nullable | `Option<Option<T>>` | None omits; Some(None) writes null |

Constructors initialize optional properties as absent. Consuming setters remain
available for composing nested structs; arrays use `Vec<T>` and their setters
accept ordinary Rust iterators. Only nullable properties have `*_null()`
setters. Public fields also permit normal Rust struct access.

Wire keys and enum spellings remain exactly as declared in CSDL. Rust fields
and setters use snake_case. Enums are closed; there is no client-style
`UnsupportedValue` variant. Ordinary navigation properties are simple resource
references. `Redfish.ExcerptCopy` properties contain separate excerpt models
such as `SensorExcerpt` or `SensorExcerptArray`, including sources that were not
selected as roots. Excerpt models contain only fields selected by `Redfish.Excerpt`
and `Redfish.ExcerptCopyOnly`, without automatic resource metadata or collection
counts. Excerpt-only fields are excluded from full resource models.

`nv-redfish-server-core` contains only shared serialization primitives:
references, finite numbers, and extension-property maps. Raw JSON values are
used for schema-authorized extensions, not for regular resource fields.

## Scope and validation

Inheritance is flattened through `common_rust::InheritedProperties`.
Read-only required fields are retained. Write-only fields and their navigation
count annotations are omitted. Conflicting Rust names, including generated
helper names, fail generation.
Routing, capabilities, calculated values, and fault injection remain application
responsibilities.

Compile-time checks cover presence and Rust types, not string formats or
cross-field constraints. `Edm.Guid`, including aliases such as `Resource.UUID`,
uses `uuid::Uuid` via `server_core::EdmGuid` and serializes as a UUID string.
Other textual EDM values currently use strings;
floating-point and decimal values use `FiniteF64`, not arbitrary-precision
decimal arithmetic. Unsupported primitives fail generation explicitly.
Bound-action descriptors and expanded navigation payloads are not generated yet.
Compilation retains the shared compiler IR's restrictions on structural cycles.
