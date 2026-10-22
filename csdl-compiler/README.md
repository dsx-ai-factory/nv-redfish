<!--
SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
SPDX-License-Identifier: Apache-2.0
-->

# nv-redfish-csdl-compiler

CSDL/OData XML compiler and Rust code generator for the
[nv-redfish](https://github.com/dsx-ai-factory/nv-redfish) Redfish stack.
It reads Redfish, Swordfish, and OEM EDMX/CSDL documents, resolves inheritance
and type references, and generates Rust for a selected schema surface.

Use this crate to generate models from your own schemas or customize schema
selection. For a Redfish client with standard service wrappers, use
[`nv-redfish`](https://crates.io/crates/nv-redfish) directly.

The compiler provides:

- Client read, update, create, excerpt, action, enum, and typedef models.
- Server response models through the `generator::server_rust` library API.
- Root selection and filters for navigation targets, OEM actions, and rigid arrays.
- A library for build scripts and a CLI for client code generation.

## Generate client models in a build script

Add the compiler as a build dependency. Generated client code uses
`nv-redfish-core` and Serde at runtime:

```toml
[dependencies]
nv-redfish-core = "0.1"
serde = { version = "1", features = ["derive"] }

[build-dependencies]
nv-redfish-csdl-compiler = "0.1"
```

Place the CSDL XML files in a local `schemas/` directory, including all documents
needed to resolve references. The compiler reads local files; it does not fetch
EDMX reference URIs. Each namespace must be defined only once across the inputs.

In `build.rs`:

```no_run
use nv_redfish_csdl_compiler::commands::{process_command, Commands};
use std::{env, error::Error, fs, path::PathBuf};

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed=schemas");
    let mut csdls = Vec::new();
    for entry in fs::read_dir("schemas")? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "xml") {
            csdls.push(path.to_string_lossy().into_owned());
        }
    }
    csdls.sort();

    process_command(&Commands::Compile {
        root: "Service".into(),
        csdls,
        output: PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is unset")?)
            .join("redfish.rs"),
        include_root_patterns: vec![],
        entity_type_patterns: vec!["ServiceRoot.*.*".parse()?],
        rigid_array_patterns: vec![],
    })?;
    Ok(())
}
```

This selects the `Service` singleton and limits navigation targets to the
`ServiceRoot` schema family. Add patterns for the services you need, or use an
empty `entity_type_patterns` list to allow all navigation targets.

Include the output in your crate:

```ignore
pub mod redfish {
    include!(concat!(env!("OUT_DIR"), "/redfish.rs"));
}
```

Read and excerpt models do not implement `Serialize` by default. Build scripts
that need it can call
`commands::process_command_with_read_model_serialization` instead of
`commands::process_command`.

## Command line

Install the executable:

```sh
cargo install nv-redfish-csdl-compiler
```

The executable is named `nv-redfish-csdl-compiler`. Compile local schema bundles
into a client model file (the shell expands the XML file globs):

```sh
nv-redfish-csdl-compiler compile \
  --root Service \
  --pattern 'ServiceRoot.*.*' \
  --pattern 'ComputerSystem.*.*' \
  --output redfish.rs \
  schemas/redfish/*.xml schemas/swordfish/*.xml
```

`--root` defaults to `Service`, and `--output` defaults to `redfish.rs`.
Repeat `--include-root-pattern` to select additional entity roots even when
they are not reachable from the singleton. Repeat `--pattern` to allow
navigation targets: for example, `SomeNamespace.*.Entity1|Entity2` selects two
entity names across versions. With no `--pattern`, all navigation targets are
allowed. `--rigid-arrays` selects properties by a pattern such as
`EthernetInterface.*.EthernetInterface/StaticNameServers`.

For OEM schemas, put root documents before `@` and documents used to resolve
their references after it:

```sh
nv-redfish-csdl-compiler compile-oem \
  --action-pattern 'ContosoAccountService.*' \
  --output oem.rs \
  schemas/oem/*.xml @ schemas/redfish/*.xml
```

All data types in the OEM root documents are selected. OEM actions must be
selected explicitly with `--action-pattern`; none are included by default.
The OEM command also supports `--pattern` and `--rigid-arrays`.
Use `compile --help` or `compile-oem --help` for all options.

## Lower-level library APIs

The compilation pipeline is available separately for custom generators:

1. Parse XML with `edmx::Edmx::parse`.
2. Build a `compiler::SchemaBundle` and compile selected roots with
   `compiler::Config`.
3. Optimize the compiled model with `optimizer::optimize`.
4. Emit client Rust with `generator::client_rust::RustGenerator`.

`features_manifest::FeaturesManifest` reads the feature-to-schema selection
manifest used by the nv-redfish build.

For serializable server response structs, use `generator::server_rust`.
It selects explicit entity or complex-type roots, flattens inheritance, keeps
ordinary navigation properties as links, and generates separate excerpt models.
Generated server code uses `nv-redfish-server-core` and Serde. See the
[server response example](https://github.com/dsx-ai-factory/nv-redfish/tree/main/examples/server-response)
for a build script and response construction.

## License

Apache-2.0.
