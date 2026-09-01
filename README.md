# nv-redfish

`nv-redfish` is a modular Rust client stack for Redfish BMC management.

The project combines generated Redfish schema types with a small transport
abstraction and optional ergonomic wrappers for common Redfish services. The
main crate is intentionally feature-gated: enable the service and OEM support
your client needs, or use `std-redfish` for a broad standard Redfish build.

## Crates

- `nv-redfish-core`
  - Transport-agnostic primitives and traits used by generated code.
  - Includes `Bmc`, `EntityTypeRef`, `NavProperty<T>`, `Action<T, R>`,
    `ODataId`, `ODataETag`, `ModificationResponse`, and Redfish session-create
    response metadata.
  - Provides common Redfish/OData value types such as date/time, duration,
    UUID, decimal, task, action, and navigation-property helpers.
  - Does not include an HTTP implementation.

- `nv-redfish-bmc-http`
  - HTTP implementation of `nv_redfish_core::Bmc`.
  - Provides `HttpBmc<C>`, `BmcCredentials`, ETag/cache handling, and the
    `HttpClient` trait.
  - The built-in reqwest client is behind the `reqwest` feature, enabled by
    default for this crate.
  - Supports custom default headers and session-token credential updates, so
    callers can use either basic credentials or a Redfish `X-Auth-Token`.

- `nv-redfish`
  - High-level Redfish API over generated schema types.
  - Exposes `ServiceRoot` and feature-gated wrappers for services such as
    accounts, chassis, systems, sessions, events, telemetry, and updates.
  - Re-exports `nv-redfish-bmc-http` as `nv_redfish::bmc_http` when the
    `bmc-http` feature is enabled.
  - Generates only the schemas required by enabled features during build.
  - Uses feature-gated patch helpers for vendor quirks and schema deviations
    observed in real BMCs.

- `nv-redfish-bmc-mock`
  - Test BMC implementation used by integration tests and examples.
  - Provides expectation helpers for GET, PATCH, POST/create, DELETE, actions,
    SSE, and Redfish session creation.

- `nv-redfish-csdl-compiler`
  - CSDL/OData XML compiler and Rust code generator.
  - Used by `nv-redfish` at build time to compile selected standard and OEM
    Redfish schemas.
  - Reads Redfish, Swordfish, and OEM CSDL/EDMX documents into a schema index,
    resolves inheritance and references, compiles a reduced intermediate model,
    optimizes it, and emits Rust.
  - Compilation is rooted at service singletons such as `Service`, plus
    feature-defined include patterns from `redfish/features.toml`.
  - Navigation targets can be limited with wildcard entity-type patterns so
    generated code contains only the reachable schema surface needed by the
    selected features.
  - Generates read, update, create, excerpt, action, enum, and typedef shapes
    consumed by `nv-redfish`.
  - CLI entry points:
    - `Compile`: compile standard CSDL from a root singleton into a Rust file.
    - `CompileOem`: compile OEM CSDL as root schemas while resolving references
      from standard CSDL files.

## Feature Flags

`nv-redfish` has no default features.

Common feature groups:

- `bmc-http`: re-export `nv-redfish-bmc-http` from `nv_redfish::bmc_http`.
- `http-extras`: enable optional HTTP client capabilities, including per-BMC operation concurrency limits; implies `bmc-http`.
- `resource-serialization`: enable Serde serialization for generated read and
  excerpt models.
- `std-redfish`: enable a broad standard Redfish surface.
- Service features: `accounts`, `assembly`, `bios`, `boot-options`,
  `chassis`, `computer-systems`, `ethernet-interfaces`, `event-service`,
  `host-interfaces`, `log-services`, `managers`, `manager-network-protocol`, `memory`,
  `network-adapters`, `network-device-functions`, `pcie-devices`, `ports`, `power`,
  `power-supplies`, `processors`, `secure-boot`, `sensors`,
  `session-service`, `storages`, `task-service`, `telemetry-service`, `thermal`,
  `update-service`.
- OEM features: `oem-ami`, `oem-dell`, `oem-hpe`, `oem-lenovo`,
  `oem-supermicro`, `oem-nvidia`, `oem-liteon`.
- OEM product features: `oem-dell-attributes`, `oem-nvidia-cper`,
  `oem-nvidia-fabrics`, `oem-nvidia-power-management`,
  `oem-nvidia-profiles`, `oem-nvidia-security`.

`oem-nvidia` carries the whole NVIDIA OEM schema set, vendored from
[NVIDIA/bmcweb](https://github.com/NVIDIA/bmcweb/tree/develop/redfish-core/schema/oem/nvidia/csdl),
and covers every NVIDIA platform including the BlueField DPU. It
compiles only the schemas belonging to the service features you already
enabled: with `chassis` you get the NVIDIA chassis schemas, with
`managers` the NVIDIA manager schemas, and so on. Families that extend
no standard service have their own `oem-nvidia-*` feature. `oem-nvidia`
on its own generates nothing.

`oem-nvidia-cper` is separate because that one schema accounts for
roughly half of the generated NVIDIA code; enable it only if you decode
Common Platform Error Records.

For smaller binaries and faster builds, enable only the service and OEM
features your client needs.

## Minimal Example

`Cargo.toml`:

```toml
[dependencies]
nv-redfish = { version = "0.1", features = ["bmc-http"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
url = "2.5"
```

Rust:

```rust
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::{BmcCredentials, CacheSettings, HttpBmc};
use nv_redfish::ServiceRoot;
use std::sync::Arc;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bmc = Arc::new(HttpBmc::new(
        Client::new()?,
        Url::parse("https://example.com")?,
        BmcCredentials::new("admin".into(), "password".into()),
        CacheSettings::default(),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    println!("Vendor: {:?}", root.vendor());
    println!("Product: {:?}", root.product());
    println!("Redfish version: {:?}", root.redfish_version());

    Ok(())
}
```

See `examples/readme-minimal` for this example as a workspace target.
See `examples/session-token` for Redfish SessionService authentication using
`X-Auth-Token`.
See `examples/task-service` for polling a Redfish Task through TaskService.
Pass a Redfish task location returned by an async operation, such as
`/redfish/v1/TaskService/Tasks/42`, with `--location`.
See `examples/metrics-oem-nvidia` for walking the metric resources of a live
BMC and reporting its NVIDIA OEM extensions. It separates "no such link" from
"no extension" from "firmware sent something the schema cannot read", and exits
non-zero on the last of those. Add `--dump` to print the raw `Oem.Nvidia`
objects.

## How It Fits Together

1. Enable features on `nv-redfish`.
2. `redfish/build.rs` invokes `nv-redfish-csdl-compiler`.
3. The compiler reads `redfish/features.toml` plus selected CSDL XML schemas
   and generates the schema module compiled into `nv-redfish`.
4. High-level wrappers use the generated types and the transport-agnostic
   `Bmc` trait.
5. Applications provide a BMC implementation, commonly `HttpBmc<Client>` from
   `nv-redfish-bmc-http`.

## Goals

- Keep the transport layer independent from the Redfish schema layer.
- Compile only the schema surface needed by enabled features.
- Support standard Redfish and selected OEM extensions.
- Keep vendor compatibility fixes isolated behind feature-gated patch helpers.

## Security
- Vulnerability disclosure: [SECURITY.md](SECURITY.md)
- Do not file public issues for security reports.

## License

See workspace `Cargo.toml`.

This project includes Redfish schema files from DMTF's
[Redfish-Publications repository](https://github.com/DMTF/Redfish-Publications/tree/main),
licensed under the
[BSD-3-Clause license](https://github.com/DMTF/Redfish-Publications/blob/main/LICENSE.md).

This project includes Swordfish schema files from SNIA's
[Swordfish-Publications repository](https://github.com/SNIA/Swordfish-Publications),
licensed under the
[BSD-3-Clause license](https://github.com/SNIA/Swordfish-Publications/blob/main/LICENSE).

## Contributing

Please see [CONTRIBUTING.md](CONTRIBUTING.md) for details.
