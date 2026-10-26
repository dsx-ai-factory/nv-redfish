<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# nv-redfish-bmc-http

Connect Redfish client code to a BMC over HTTP. `HttpBmc` handles resource URLs,
authentication, conditional reads, and response decoding through the `Bmc`
interface from [`nv-redfish-core`](https://crates.io/crates/nv-redfish-core).
Use it with generated models, generic resource code, or the service wrappers in
[`nv-redfish`](https://crates.io/crates/nv-redfish).

The built-in client uses reqwest. To use another HTTP library, implement
`HttpClient` and pass it to `HttpBmc`.

## Read a resource

```toml
[dependencies]
nv-redfish-bmc-http = "0.1"
nv-redfish-core = "0.1"
tokio = { version = "1", features = ["macros", "rt"] }
url = "2"
```

```no_run
use nv_redfish_bmc_http::reqwest::Client;
use nv_redfish_bmc_http::{BmcCredentials, CacheSettings, HttpBmc};
use nv_redfish_core::{Bmc, ODataId, Raw};
use url::Url;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bmc = HttpBmc::new(
        Client::new()?,
        Url::parse("https://bmc.example.com")?,
        BmcCredentials::new("admin".into(), "password".into()),
        CacheSettings::default(),
    );

    let root = bmc.get::<Raw>(&ODataId::service_root()).await?;
    println!("Redfish version: {}", root.value()["RedfishVersion"]);
    Ok(())
}
```

`Raw` reads the resource as JSON. For typed reads, use generated entities that
implement `EntityTypeRef`. Navigation properties and operations on those models
use the same `Bmc` interface. Wrap the transport in `Arc` when sharing it with
service wrappers or concurrent tasks.

## Authentication and headers

`BmcCredentials` supports username/password credentials, Redfish session tokens,
and anonymous access. Use `BmcCredentials::token(token)` after creating a session;
`HttpBmc::set_credentials` changes authentication for subsequent requests while
preserving the cache.

`Bmc::create_session` returns the session entity, authentication token, and
session URI together in `SessionCreateResponse`. The application decides when
to install the token and delete the session.

Use `HttpBmc::with_custom_headers` to add headers to requests, such as a
vendor-specific header. `reqwest::ClientParams` also supports default headers
for the underlying HTTP client.

## Configure the HTTP client

Create a client with `Client::with_params(ClientParams::new()...)` to configure
request and connection timeouts, connection pooling, redirects, and SSE limits.

`ClientParams::retry` accepts a `RetryPolicy` whose classifier examines each
request and received response. Set `max_retries` to allow extra attempts and
`delay` for a fixed pause between them. Transport errors return immediately;
streaming upload bodies are sent once. Choose the methods and response statuses
that your application can retry.

The default transport uses rustls with the `ring` cryptographic provider.
Choose another backend by disabling default features:

```toml
nv-redfish-bmc-http = { version = "0.1", default-features = false, features = ["reqwest-native-tls"] }
```

| Option | Purpose |
| --- | --- |
| `reqwest` | Default client with rustls, `ring`, and webpki roots. |
| `reqwest-native-tls` | Client using the platform's TLS library. |
| `reqwest-rustls-no-provider` | Client using rustls with a provider installed by the application. |
| `reqwest-client` | Base reqwest client; TLS is added by the options above. |
| `http-extras` | Concurrency limiting for BMC operations. |

With provider-free rustls, install a process-default
`rustls::crypto::CryptoProvider` before constructing the client. When both TLS
backends are compiled, `ClientParams::use_rust_tls` selects rustls by default;
set it to `false` to choose native TLS.

## Cache and concurrency

`HttpBmc` caches resource representations carrying an `ETag` and revalidates them
with `If-None-Match`. A `304 Not Modified` response reuses the cached value.
`CacheSettings::default()` holds up to 100 entries; use
`CacheSettings::with_capacity(0)` to disable caching and conditional reads.

Repaired reads apply a `Repair` before deserialization. Use the same repair for
every read of a resource through one caching transport, because a later read
may reuse the earlier result. Platform repairs are provided by
[`nv-redfish-quirks`](https://crates.io/crates/nv-redfish-quirks).

With `http-extras`, call `HttpBmc::with_request_concurrency_limit` with a
`NonZeroUsize` to obtain a `ConcurrencyLimitedBmc`. The limit covers each logical
operation, including retries. Requests wait for an available permit.

## Writes, uploads, and events

Create, update, delete, action, and task-poll methods return
`ModificationResponse<T>`. Handle entity, asynchronous task, success-message,
and empty outcomes as described in
[`nv-redfish-core`](https://crates.io/crates/nv-redfish-core).

`Bmc::multipart_update` uploads firmware from a streamed reader with update
parameters and optional OEM parts. `Bmc::stream` decodes SSE payloads;
`Bmc::stream_events` also preserves event IDs and accepts a last event ID when
reopening a stream. Configure event size and idle limits through
`ClientParams::sse_max_event_bytes` and `sse_idle_timeout`.

The reqwest client ignores unknown SSE fields, uses the last valid value
when IDs repeat, and clears the resume cursor for an `id` without a colon.
Do not enable `sse-stream/strict-fields`: Cargo feature unification can make
the client reject unknown fields.

Service-provided action, monitor, upload, and event-stream URIs are resolved
against the configured BMC. Cross-origin URI references and redirects are
rejected by the built-in client.

## Use another HTTP library

Implement `HttpClient` and make its error type implement `BmcError`,
`CacheableError`, and `RequestError`. Use `decode_modification` and `decode_poll` to interpret
responses with the same rules as the reqwest client. Report typed decode
failures through `BmcError::into_decode_error` so repaired reads can distinguish
them from transport failures.

## License

Apache-2.0.
