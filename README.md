# nv-redfish

[![License: Apache-2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)

`nv-redfish` is a modular Rust client stack for Redfish BMC management.

It is for Rust developers building infrastructure management tools that need
typed access to BMC resources. Handwritten JSON models and device-specific
response handling are difficult to keep consistent across firmware versions.
This project generates types from Redfish schemas and keeps compatibility
repairs separate from application logic.

The project combines generated Redfish schema types with a small transport
abstraction and optional ergonomic wrappers for common Redfish services. The
main crate is intentionally feature-gated: enable the service and OEM support
your client needs, or use `std-redfish` for a broad standard Redfish build.

The schema layer integrates DMTF Redfish, SNIA Swordfish, and selected OEM
extensions; the optional HTTP transport uses reqwest and Tokio. The project
provides client APIs and server response serialization primitives. Applications
supply BMC access, credentials, and management workflows; the response models
do not provide an HTTP server.

## Quick Start

Install Git, a C toolchain for native dependencies, and Rust through rustup.
The checked-in [toolchain file](rust-toolchain.toml) selects Rust 1.95.0.
Clone the schema submodules along with the source, then check the minimal client
and run a local response example:

```sh
git clone --recurse-submodules https://github.com/dsx-ai-factory/nv-redfish.git
cd nv-redfish
cargo check -p readme-minimal
cargo run -p server-response
```

`cargo check` verifies the client example without contacting a BMC. The response
example prints generated Redfish resource JSON and does not need hardware or
credentials. To check a broader standard client with NVIDIA OEM extensions:

```sh
cargo check -p nv-redfish --features std-redfish,oem-nvidia
```

Choose individual features for a smaller generated API. The
[Makefile](Makefile) defines the CI feature combinations; `make ci-features`
checks those combinations, and `make ci` also runs lint and test groups.

## Crates

- `nv-redfish-core`
  - Transport-agnostic primitives and traits used by generated code.
  - Includes `Bmc`, `EntityTypeRef`, `NavProperty<T>`, `Action<T, R>`,
    `ODataId`, `ODataETag`, `ModificationResponse`, and Redfish session-create
    response metadata.
  - Provides common Redfish/OData value types such as date/time, duration,
    UUID, decimal, task, action, and navigation-property helpers.
  - Defines `Repair`, a rewrite of a document before it is deserialized, and
    the `Bmc` reads that apply one.
  - Does not include an HTTP implementation.

- `nv-redfish-bmc-http`
  - HTTP implementation of `nv_redfish_core::Bmc`.
  - Provides `HttpBmc<C>`, `BmcCredentials`, ETag/cache handling, and the
    `HttpClient` trait.
  - The built-in reqwest client is behind the `reqwest` feature, enabled by
    default for this crate, which uses rustls with `ring`. The
    `reqwest-native-tls` feature builds it with the platform's TLS library
    instead, and `reqwest-rustls-no-provider` with rustls and a
    `CryptoProvider` the application installs; neither compiles in `ring`.
    Because `ClientParams::use_rust_tls` defaults to `true`, enabling both
    `reqwest-native-tls` and `reqwest-rustls-no-provider` (without `reqwest`)
    causes `Client::new()` to panic unless a `CryptoProvider` is installed (or
    `use_rust_tls` is set to `false` via `Client::with_params`).
  - Provides `decode_modification` and `decode_poll`, so an `HttpClient` over
    another HTTP library decodes the responses to modifications and polls as
    the reqwest client does, success messages included. `success_code` and
    `strict_success_code` tell what a success message reports, for a caller
    that must not count `NoOperation` or a success with warnings.
  - Supports custom default headers and session-token credential updates, so
    callers can use either basic credentials or a Redfish `X-Auth-Token`.
  - Applies a `Repair` in its own decode. Its cache keeps each resource as
    the last full read decoded it, so reads of one resource should share a
    repair.

- `nv-redfish-quirks`
  - Platform quirks of real BMCs, kept apart from the generated model.
  - Classifies an endpoint from its service root and holds the document
    repairs its platform needs, keyed by the document's `@odata.type`.
  - Provides `CompatBmc`, a `Bmc` over another that applies those repairs to
    every document it reads, event stream payloads included.
  - Provides `RulesBmc`, a transport wrapper that adds the caller's
    `UserRule`s. The rules are the transport's for its whole life, so new
    rules take a new transport.

- `nv-redfish-server-core`
  - Shared serialization primitives for generated server response structs.
  - Provides resource references, finite numbers, and schema extension properties.
  - Does not depend on the client core, an HTTP framework, or specific schemas.

- `nv-redfish`
  - High-level Redfish API over generated schema types.
  - Exposes `ServiceRoot` and feature-gated wrappers for services such as
    accounts, chassis, systems, sessions, events, telemetry, and updates.
  - Re-exports `nv-redfish-bmc-http` as `nv_redfish::bmc_http` when one of
    `bmc-http`, `bmc-http-native-tls`, `bmc-http-rustls-no-provider`, or
    `http-extras` is enabled (`http-extras` alone does not enable the
    `reqwest` client).
  - Generates only the schemas required by enabled features during build.
  - Reads every resource through `CompatBmc`, so its wrappers and schema
    types read directly get the same platform repairs; a `RulesBmc`
    transport adds the caller's.

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

- `bmc-http`: re-export `nv-redfish-bmc-http` from `nv_redfish::bmc_http`, with
  its reqwest client using rustls and `ring`.
- `bmc-http-native-tls`, `bmc-http-rustls-no-provider`: the same with the
  reqwest client using the platform's TLS library, or rustls with the
  application's `CryptoProvider`, without `ring`. If both are enabled without
  `bmc-http`, `bmc_http::reqwest::Client::new()` still uses rustls by default
  (`ClientParams::use_rust_tls` defaults to `true`) and panics unless a
  `CryptoProvider` is installed.
- `http-extras`: re-export `nv-redfish-bmc-http` from `nv_redfish::bmc_http`
  with optional HTTP client capabilities, including per-BMC operation
  concurrency limits. Does not imply `bmc-http` or enable the `reqwest`
  client; combine with `bmc-http`, `bmc-http-native-tls`, or
  `bmc-http-rustls-no-provider` when using `bmc_http::reqwest`.
- `resource-serialization`: enable Serde serialization for generated read and
  excerpt models.
- `std-redfish`: enable a broad standard Redfish surface.
- Service features: `accounts`, `assembly`, `bios`, `boot-options`,
  `certificates`, `chassis`, `component-integrity`, `computer-systems`,
  `ethernet-interfaces`, `event-service`, `fabrics`,
  `host-interfaces`, `log-services`, `managers`, `manager-network-protocol`, `memory`,
  `network-adapters`, `network-device-functions`, `pcie-devices`, `ports`, `power`,
  `power-supplies`, `processors`, `secure-boot`, `serial-interfaces`, `sensors`,
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
`managers` the NVIDIA manager schemas, and with `fabrics` the NVIDIA Fabric and
Switch schemas and wrappers.
Thus `std-redfish` + `oem-nvidia` includes NVIDIA
Fabric/Switch support without an additional flag. Families that extend
no standard service have their own `oem-nvidia-*` feature. `oem-nvidia`
on its own generates nothing.

The `oem-nvidia-fabrics` feature provides additional schemas for switch power
modes, ports, and port/NVLink metrics.

The `fabrics` feature, included in `std-redfish`, enables both Fabric and Switch
types in `nv_redfish::fabric`, including `Fabric::switches()` traversal.
Add `oem-nvidia` for NVIDIA extensions.
Enable `ports` as well to use `Switch::ports()`.

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

See [`examples/readme-minimal`](examples/readme-minimal) for this example as a
workspace target. Its URL and credentials are placeholders: running it requires
a reachable BMC and valid credentials, so the quick start checks it without
running it.
See [`examples/server-response`](examples/server-response) for CSDL-generated
server response structs. Run `cargo run -p server-response` to print example
Chassis and ChassisCollection JSON responses.
See [`examples/session-token`](examples/session-token) for Redfish SessionService authentication using
`X-Auth-Token`.
See [`examples/task-service`](examples/task-service) for polling a Redfish Task through TaskService.
Pass a Redfish task location returned by an async operation, such as
`/redfish/v1/TaskService/Tasks/42`, with `--location`.
See [`examples/metrics-oem-nvidia`](examples/metrics-oem-nvidia) for walking the metric resources of a live
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
- Keep vendor compatibility fixes in one place, `nv-redfish-quirks`, as facts
  about the documents a device sends rather than changes to the schema.

## Documentation and Support

- [Core types and transport traits](core/README.md)
- [HTTP transport and TLS configuration](bmc-http/README.md)
- [CSDL compiler and code generation](csdl-compiler/README.md)
- [Server serialization primitives](server-core/README.md) and
  [generated response example](examples/server-response/README.md)
- [Dispatcher and scheduling](dispatcher/README.md)
- [GitHub Issues](https://github.com/dsx-ai-factory/nv-redfish/issues) for usage
  questions, bug reports, and feature requests. Include the enabled features,
  firmware version, and a sanitized response when reporting compatibility issues.
- [Contributing guide](CONTRIBUTING.md) and
  [proposed Code of Conduct](CODE_OF_CONDUCT.md). The conduct proposal awaits
  owner confirmation of its terms, enforcement responsibilities, and reporting
  channel before adoption.

## Security
- Vulnerability disclosure: [SECURITY.md](SECURITY.md)
- Do not file public issues for security reports.

## License

The project uses [Apache-2.0](LICENSE), as declared in the
[workspace manifest](Cargo.toml).

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
