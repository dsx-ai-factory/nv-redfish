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

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use nv_redfish_csdl_compiler::commands::process_command;
use nv_redfish_csdl_compiler::commands::process_command_with_read_model_serialization;
use nv_redfish_csdl_compiler::commands::Commands;

use syn::punctuated::Punctuated;

const COMPILER: &str = env!("CARGO_BIN_EXE_nv-redfish-csdl-compiler");
const SERVICE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/service.xml");
const OEM: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/oem.xml");
const ADDITIONAL_ROOT_PATTERN: &str = "ServiceRoot.*.AdditionalRoot";
const OEM_ACTION_PATTERN: &str = "ExampleOem.Reset";

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(case: &str) -> Self {
        let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
        let name = format!("compiler-output-{}-{case}", std::process::id());

        let mut path = base.join(&name);
        let mut suffix = 0;

        loop {
            match fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    suffix += 1;
                    path = base.join(format!("{name}-{suffix}"));
                }
                Err(error) => panic!("failed to create {}: {error}", path.display()),
            }
        }
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn compile_writes_selected_roots_and_prints_the_output_path_once() {
    let directory = TestDirectory::new("compile");
    let output = directory.0.join("service.rs");

    let result = Command::new(COMPILER)
        .args(["compile", "--root", "Service", "--include-root-pattern"])
        .arg(ADDITIONAL_ROOT_PATTERN)
        .arg("--output")
        .arg(&output)
        .arg(SERVICE)
        .output()
        .unwrap();

    assert!(result.status.success(), "{:?}", result);

    let rust = fs::read_to_string(&output).unwrap();

    assert!(rust.contains("pub struct ServiceRoot {"));
    assert!(rust.contains("pub struct AdditionalRoot {"));

    assert_eq!(
        result.stdout,
        format!("{} file has been written\n", output.display()).as_bytes()
    );
}

#[test]
fn compile_oem_writes_selected_actions_and_prints_the_output_path_once() {
    let directory = TestDirectory::new("compile-oem");
    let output = directory.0.join("oem.rs");

    let result = Command::new(COMPILER)
        .args(["compile-oem", "--action-pattern", OEM_ACTION_PATTERN])
        .arg("--output")
        .arg(&output)
        .args([OEM, "@", SERVICE])
        .output()
        .unwrap();

    assert!(result.status.success(), "{:?}", result);

    let rust = fs::read_to_string(&output).unwrap();

    assert!(rust.contains("pub struct AdapterInfo {"));
    assert!(rust.contains("pub struct ServiceRootResetAction {"));
    assert!(!rust.contains("SkipAction"));
    assert!(!rust.contains("pub struct ServiceRoot {"));

    assert_eq!(
        result.stdout,
        format!("{} file has been written\n", output.display()).as_bytes()
    );
}

#[test]
fn compile_preserves_existing_output_when_the_singleton_is_unknown() {
    let directory = TestDirectory::new("unknown-root");
    let output = directory.0.join("existing.rs");
    let existing = "previous output\n";

    fs::write(&output, existing).unwrap();

    let result = Command::new(COMPILER)
        .args(["compile", "--root", "Servcie", "--include-root-pattern"])
        .arg(ADDITIONAL_ROOT_PATTERN)
        .arg("--output")
        .arg(&output)
        .arg(SERVICE)
        .output()
        .unwrap();

    assert!(!result.status.success(), "{:?}", result);
    assert!(result.stdout.is_empty(), "{:?}", result);

    let diagnostic = String::from_utf8(result.stderr).unwrap();

    assert!(
        diagnostic.contains("root singleton not found: Servcie"),
        "{}",
        diagnostic
    );

    assert_eq!(fs::read_to_string(&output).unwrap(), existing);
}

#[test]
fn compile_reports_closed_stdout_after_writing_rust() {
    let directory = TestDirectory::new("closed-stdout");
    let output = directory.0.join("service.rs");
    let (reader, writer) = io::pipe().unwrap();

    drop(reader);

    let result = Command::new(COMPILER)
        .args(["compile", "--root", "Service", "--output"])
        .arg(&output)
        .arg(SERVICE)
        .stdout(Stdio::from(writer))
        .output()
        .unwrap();

    assert_eq!(result.status.code(), Some(1), "{result:?}");

    let diagnostic = String::from_utf8(result.stderr).unwrap();

    assert!(diagnostic.contains("Io(\"stdout\","), "{}", diagnostic);

    let rust = fs::read_to_string(&output).unwrap();

    assert!(syn::parse_file(&rust).is_ok());
    assert!(rust.contains("pub struct ServiceRoot {"));
}

#[test]
fn library_commands_forward_read_model_serialization() {
    let directory = TestDirectory::new("serialization");
    let output = directory.0.join("models.rs");

    let compile = Commands::Compile {
        root: "Service".into(),
        include_root_patterns: Vec::new(),
        csdls: vec![SERVICE.into()],
        output: output.clone(),
        entity_type_patterns: Vec::new(),
        rigid_array_patterns: Vec::new(),
    };

    let compile_oem = Commands::CompileOem {
        root_csdls: vec![OEM.into()],
        resolve_csdls: vec![SERVICE.into()],
        output: output.clone(),
        entity_type_patterns: Vec::new(),
        action_patterns: vec![OEM_ACTION_PATTERN.parse().unwrap()],
        rigid_array_patterns: Vec::new(),
    };

    let cases = [
        (compile, "service_root", "ServiceRoot"),
        (compile_oem, "example_oem", "AdapterInfo"),
    ];

    for (command, namespace, model) in cases {
        for serialize in [false, true] {
            let messages = if serialize {
                process_command_with_read_model_serialization(&command)
            } else {
                process_command(&command)
            }
            .unwrap();

            let rust = fs::read_to_string(&output).unwrap();
            let syntax = syn::parse_file(&rust).unwrap();

            let generated_model = syntax
                .items
                .iter()
                .find_map(|item| match item {
                    syn::Item::Mod(module) if module.ident == namespace => module.content.as_ref(),
                    _ => None,
                })
                .and_then(|(_, items)| {
                    items.iter().find_map(|item| match item {
                        syn::Item::Struct(item) if item.ident == model => Some(item),
                        _ => None,
                    })
                })
                .expect("expected model was generated in the selected namespace");

            let derives_serialize = generated_model
                .attrs
                .iter()
                .filter(|attr| attr.path().is_ident("derive"))
                .any(|attr| {
                    let derives = attr
                        .parse_args_with(Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
                        .unwrap();

                    derives
                        .iter()
                        .filter_map(|derive| derive.segments.last())
                        .any(|segment| segment.ident == "Serialize")
                });

            assert_eq!(derives_serialize, serialize, "{command:?}");

            assert_eq!(
                messages,
                [format!("{} file has been written", output.display())]
            );
        }
    }
}
