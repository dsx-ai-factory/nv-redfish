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

//! Integration tests for NVIDIA Processor OEM payloads.

use nv_redfish::computer_system::Processor;
use nv_redfish::oem::nvidia::NvidiaProcessor;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::test;

const GPU_TYPE: &str = "#NvidiaProcessor.v1_7_0.NvidiaGPU";

#[test]
async fn gpu_oem_payload_has_typed_fields() -> Result<(), Box<dyn StdError>> {
    let processor = get_processor(Some(json!({
        "Nvidia": {
            ODATA_TYPE: GPU_TYPE,
            "MIGModeEnabled": true,
            "CCModeEnabled": false,
            "TotalNumberNVLinks": 18,
        }
    })))
    .await?;

    let Some(NvidiaProcessor::Gpu(gpu)) = processor.oem_nvidia()? else {
        panic!("processor must expose the GPU shape");
    };
    assert_eq!(gpu.mig_mode_enabled.flatten(), Some(true));
    assert_eq!(gpu.cc_mode_enabled.flatten(), Some(false));
    assert_eq!(gpu.total_number_nv_links.flatten(), Some(18));
    Ok(())
}

#[test]
async fn lpu_oem_payload_has_typed_fields() -> Result<(), Box<dyn StdError>> {
    let processor = get_processor(Some(json!({
        "Nvidia": {
            ODATA_TYPE: "#NvidiaProcessor.v1_7_0.NvidiaLPU",
            "BootStatusCode": 7,
            "VoltageBinNumber": 3,
        }
    })))
    .await?;

    let Some(NvidiaProcessor::Lpu(lpu)) = processor.oem_nvidia()? else {
        panic!("processor must expose the LPU shape");
    };
    assert_eq!(lpu.boot_status_code.flatten(), Some(7));
    assert_eq!(lpu.voltage_bin_number.flatten(), Some(3));
    Ok(())
}

#[test]
async fn missing_or_null_nvidia_oem_payload_is_absent() -> Result<(), Box<dyn StdError>> {
    for oem in [None, Some(json!({ "Nvidia": null }))] {
        let processor = get_processor(oem).await?;
        assert!(processor.oem_nvidia()?.is_none());
    }
    Ok(())
}

#[test]
async fn unknown_or_missing_type_keeps_generic_properties() -> Result<(), Box<dyn StdError>> {
    for odata_type in [None, Some("#NvidiaProcessor.v9_9_0.NvidiaSomethingElse")] {
        let mut nvidia = json!({ "MIGModeEnabled": true });
        if let Some(odata_type) = odata_type {
            nvidia[ODATA_TYPE] = json!(odata_type);
        }
        let processor = get_processor(Some(json!({ "Nvidia": nvidia }))).await?;
        let Some(NvidiaProcessor::Generic(generic)) = processor.oem_nvidia()? else {
            panic!("unknown type must retain generic processor properties");
        };
        assert_eq!(generic.mig_mode_enabled.flatten(), Some(true));
    }
    Ok(())
}

#[test]
async fn malformed_declared_gpu_is_an_error() -> Result<(), Box<dyn StdError>> {
    let processor = get_processor(Some(json!({
        "Nvidia": {
            ODATA_TYPE: GPU_TYPE,
            "MIGModeEnabled": "invalid",
        }
    })))
    .await?;
    assert!(processor.oem_nvidia().is_err());
    Ok(())
}

async fn get_processor(oem: Option<Value>) -> Result<Processor<Bmc>, Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/System");
    let processors_id = format!("{system_id}/Processors");
    let processor_id = format!("{processors_id}/GPU_0");

    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": { "ExpandQuery": { "NoLinks": true } },
            "Systems": { ODATA_ID: &systems_id },
            "Links": {
                "Sessions": { ODATA_ID: format!("{root_id}/SessionService/Sessions") }
            },
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::expand(
        &systems_id,
        json!({
            ODATA_ID: &systems_id,
            ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
            "Id": "Systems",
            "Name": "Computer System Collection",
            "Members": [{
                ODATA_ID: &system_id,
                ODATA_TYPE: "#ComputerSystem.v1_19_0.ComputerSystem",
                "Id": "System",
                "Name": "System",
                "Processors": { ODATA_ID: &processors_id },
            }],
        }),
    ));
    let system = root
        .systems()
        .await?
        .expect("service root must expose systems")
        .members()
        .await?
        .into_iter()
        .next()
        .expect("system must exist");

    let mut processor = json!({
        ODATA_ID: &processor_id,
        ODATA_TYPE: "#Processor.v1_18_0.Processor",
        "Id": "GPU_0",
        "Name": "GPU 0",
    });
    if let Some(oem) = oem {
        processor["Oem"] = oem;
    }
    bmc.expect(Expect::expand(
        &processors_id,
        json!({
            ODATA_ID: &processors_id,
            ODATA_TYPE: "#ProcessorCollection.ProcessorCollection",
            "Id": "Processors",
            "Name": "Processor Collection",
            "Members": [processor],
        }),
    ));
    Ok(system
        .processors()
        .await?
        .expect("system must expose processors")
        .into_iter()
        .next()
        .expect("processor must exist"))
}
