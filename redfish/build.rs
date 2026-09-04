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

use nv_redfish_csdl_compiler::commands::process_command as process_command_default;
use nv_redfish_csdl_compiler::commands::process_command_with_read_model_serialization;
use nv_redfish_csdl_compiler::commands::Commands;
use nv_redfish_csdl_compiler::commands::DEFAULT_ROOT;
use nv_redfish_csdl_compiler::features_manifest::FeaturesManifest;
use nv_redfish_schema::cargo_feature_enabled;
use nv_redfish_schema::glob_oem_xml;
use nv_redfish_schema::oem_schema;
use nv_redfish_schema::out_dir;
use nv_redfish_schema::redfish_schema;
use nv_redfish_schema::rerun_for;
use nv_redfish_schema::run_with_big_stack;
use nv_redfish_schema::swordfish_schema;
use std::error::Error as StdError;
use std::fs::File;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    run_with_big_stack(run)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn run() -> Result<(), Box<dyn StdError>> {
    let features_manifest = PathBuf::from("features.toml");
    let manifest = FeaturesManifest::read(&features_manifest)?;
    rerun_for([&features_manifest]);

    let serialize_read_models = cargo_feature_enabled("resource-serialization");

    let process_command: fn(&Commands) -> Result<Vec<String>, nv_redfish_csdl_compiler::Error> =
        if serialize_read_models {
            process_command_with_read_model_serialization
        } else {
            process_command_default
        };

    let redfish_csdl: [&str; 5] = [
        "Settings_v1.xml",
        "Message_v1.xml",
        "Resource_v1.xml",
        "ResolutionStep_v1.xml",
        "ActionInfo_v1.xml",
    ];

    // ================================================================================
    // Compile standard DMTF schema

    let target_features = manifest
        .all_features()
        .into_iter()
        .filter(|f| cargo_feature_enabled(f))
        .collect::<Vec<_>>();

    let out_dir = out_dir();
    let service_root: [&str; 1] = ["ServiceRoot_v1.xml"];
    let service_root_patterns = ["ServiceRoot.*.*"]
        .iter()
        .map(|v| v.parse())
        .collect::<Result<Vec<_>, _>>()
        .expect("must be successfuly parsed");
    let features = manifest.collect(&target_features);

    let standard_csdls = redfish_csdl
        .iter()
        .copied()
        .map(redfish_schema)
        .chain(features.csdl_files.iter().map(|f| redfish_schema(f)))
        .chain(
            features
                .swordfish_csdl_files
                .iter()
                .map(|f| swordfish_schema(f)),
        )
        .collect::<std::collections::HashSet<_>>();

    let csdls = standard_csdls
        .iter()
        .cloned()
        .chain(service_root.iter().copied().map(redfish_schema))
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();

    rerun_for(&csdls);

    process_command(&Commands::Compile {
        root: DEFAULT_ROOT.into(),
        include_root_patterns: features.root_patterns.into_iter().cloned().collect(),
        output: out_dir.join("redfish.rs"),
        csdls,
        entity_type_patterns: service_root_patterns
            .iter()
            .chain(features.patterns)
            .cloned()
            .collect(),
        rigid_array_patterns: features.rigid_array_patterns.into_iter().cloned().collect(),
    })?;

    // ================================================================================
    // Compile OEM-specific schemas

    let vendors = manifest
        .all_vendors()
        .into_iter()
        .filter(|v| cargo_feature_enabled(&format!("oem-{v}")))
        .collect::<Vec<_>>();

    for v in vendors {
        let vendor_features = manifest
            .all_vendor_features(v)
            .into_iter()
            .filter(|name| cargo_feature_enabled(name))
            .collect::<Vec<_>>();

        let output = out_dir.join(format!("oem-{v}.rs"));
        if vendor_features.is_empty() {
            // Just create empty output file:
            File::create(output)?;
            continue;
        }

        let oem_features = manifest.collect_vendor_features(v, &vendor_features);

        let root_names = oem_features
            .root_csdls
            .iter()
            .map(|f| f.as_str())
            .collect::<std::collections::HashSet<_>>();

        // A vendor's schemas reference each other, but only those
        // selected by the enabled features are compiled. Offer the rest
        // for type resolution so a feature never has to name the
        // transitive closure of its own dependencies.
        let unselected_oem = glob_oem_xml(v)
            .into_iter()
            .filter(|f| !root_names.contains(file_name(f)))
            .collect::<Vec<_>>();

        let root_csdls = oem_features
            .root_csdls
            .iter()
            .map(|f| oem_schema(v, f))
            .collect::<Vec<_>>();

        let resolve_csdls = standard_csdls
            .iter()
            .cloned()
            .chain(oem_features.resolve_csdls.iter().map(|f| redfish_schema(f)))
            .chain(
                oem_features
                    .swordfish_resolve_csdls
                    .iter()
                    .map(|f| swordfish_schema(f)),
            )
            .chain(unselected_oem)
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        rerun_for(root_csdls.iter().chain(resolve_csdls.iter()));

        process_command(&Commands::CompileOem {
            output,
            root_csdls,
            resolve_csdls,
            entity_type_patterns: oem_features.patterns.into_iter().cloned().collect(),
            action_patterns: oem_features.action_patterns.into_iter().cloned().collect(),
            rigid_array_patterns: vec![],
        })?;
    }
    Ok(())
}
