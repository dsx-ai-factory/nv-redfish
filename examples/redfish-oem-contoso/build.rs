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
use nv_redfish_csdl_compiler::Error;
use nv_redfish_schema::glob_oem_xml;
use nv_redfish_schema::glob_redfish_xml;
use nv_redfish_schema::out_dir;
use nv_redfish_schema::rerun_for;
use nv_redfish_schema::run_with_big_stack;

fn main() -> Result<(), String> {
    run_with_big_stack(run)
}

fn run() -> Result<(), Error> {
    let root_csdls = glob_oem_xml("contoso");
    let resolve_csdls = glob_redfish_xml();

    rerun_for(root_csdls.iter().chain(resolve_csdls.iter()));

    process_command(&Commands::CompileOem {
        output: out_dir().join("redfish_oem_contoso.rs"),
        root_csdls,
        resolve_csdls,
        entity_type_patterns: ["ServiceRoot.*.*", "LogEntry.*"]
            .iter()
            .map(|v| v.parse())
            .collect::<Result<Vec<_>, _>>()
            .expect("must be successfuly parsed"),
        action_patterns: ["ContosoAccountService.*"]
            .iter()
            .map(|v| v.parse())
            .collect::<Result<Vec<_>, _>>()
            .expect("must be successfuly parsed"),
        rigid_array_patterns: vec![],
    })?;
    Ok(())
}
