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
use nv_redfish_schema::out_dir;
use nv_redfish_schema::redfish_schema;
use nv_redfish_schema::rerun_for;
use std::env::var;
use std::error::Error as StdError;

fn main() -> Result<(), Box<dyn StdError>> {
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_REQWEST");
    if var("CARGO_FEATURE_REQWEST").is_err() {
        return Ok(());
    }

    let root_csdls = ["RedfishError_v1.xml", "Message_v1.xml"]
        .iter()
        .map(|f| redfish_schema(f))
        .collect::<Vec<_>>();
    let resolve_csdls = [
        "Settings_v1.xml",
        "Resource_v1.xml",
        "ResolutionStep_v1.xml",
        "ActionInfo_v1.xml",
    ]
    .iter()
    .map(|f| redfish_schema(f))
    .collect::<Vec<_>>();

    rerun_for(root_csdls.iter().chain(resolve_csdls.iter()));

    process_command(&Commands::CompileOem {
        output: out_dir().join("redfish.rs"),
        root_csdls,
        resolve_csdls,
        entity_type_patterns: Vec::new(),
        action_patterns: Vec::new(),
        rigid_array_patterns: Vec::new(),
    })?;

    Ok(())
}
