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

use std::path::PathBuf;

use nv_redfish_csdl_compiler::compiler::QualifiedName;
use nv_redfish_csdl_compiler::edmx::QualifiedTypeName;
use nv_redfish_csdl_compiler::generator::server_rust::{Builder, Config, Input};
use nv_redfish_schema::{glob_redfish_xml, out_dir, rerun_for};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sources = glob_redfish_xml();
    let config = Config {
        constructor_max_param_number_threshold: 3,
    };
    rerun_for(&sources);
    let root_names = [
        "ServiceRoot.ServiceRoot",
        "ChassisCollection.ChassisCollection",
        "Chassis.Chassis",
        "SensorCollection.SensorCollection",
        "Sensor.Sensor",
        "ThermalSubsystem.ThermalSubsystem",
        "ThermalMetrics.ThermalMetrics",
    ]
    .iter()
    .map(|name| name.parse())
    .collect::<Result<Vec<QualifiedTypeName>, _>>()
    .map_err(|error| error.to_string())?;
    let roots: Vec<_> = root_names.iter().map(QualifiedName::from).collect();
    let paths: Vec<_> = sources.iter().map(PathBuf::from).collect();
    let generated = Builder::new(&roots)
        .config(config)
        .generate(Input::Files(&paths))?;
    std::fs::write(out_dir().join("redfish_server.rs"), generated)?;
    Ok(())
}
