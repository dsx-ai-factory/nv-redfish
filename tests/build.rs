// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

use nv_redfish_csdl_compiler::commands::process_command;
use nv_redfish_csdl_compiler::commands::Commands;
use nv_redfish_csdl_compiler::commands::DEFAULT_ROOT;
use nv_redfish_csdl_compiler::Error;
use nv_redfish_schema::out_dir;
use nv_redfish_schema::rerun_for;
use std::fs;
use std::path::Path;

fn main() -> Result<(), Error> {
    let base_csdls = ["./schemas/base/schema.xml"]
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();

    rerun_for(&base_csdls);

    process_command(&Commands::Compile {
        root: DEFAULT_ROOT.into(),
        output: out_dir().join("base_tests.rs"),
        csdls: base_csdls,
        entity_type_patterns: vec![],
        include_root_patterns: ["ServiceRoot.*.RootSetOnlyComplexType", "Inheritance.*"]
            .iter()
            .map(|pattern| pattern.parse().expect("valid root-set type pattern"))
            .collect(),
        rigid_array_patterns: vec!["ServiceRoot.*.ServiceRoot/RigidArrayValues"
            .parse()
            .expect("valid rigid array pattern")],
    })?;

    integration_tests();

    Ok(())
}

/// Collect every `tests/*.rs` into the one `integration` test binary.
///
/// Each separate test binary instantiates nv-redfish's generic code again,
/// which made those binaries the bulk of the build. Listing the directory here
/// means a new test file cannot be left out of the binary by mistake.
fn integration_tests() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    println!("cargo:rerun-if-changed={}", dir.display());

    let mut files = fs::read_dir(&dir)
        .expect("tests directory")
        .map(|entry| entry.expect("tests directory entry").path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "rs")
                && path.file_name().is_some_and(|name| name != "main.rs")
        })
        .collect::<Vec<_>>();
    files.sort();

    let modules = files
        .iter()
        .map(|path| {
            let stem = path.file_stem().expect("file stem").to_string_lossy();
            let path = path.to_str().expect("UTF-8 test path");
            format!("#[path = {path:?}]\nmod {};\n", stem.replace('-', "_"))
        })
        .collect::<String>();

    fs::write(out_dir().join("integration_tests.rs"), modules)
        .expect("write integration test modules");
}
