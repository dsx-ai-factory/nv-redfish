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
    Ok(())
}
