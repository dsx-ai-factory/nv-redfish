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
//! Integration tests for NVIDIA OEM extensions on metric resources.

use nv_redfish::chassis::Chassis;
use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::oem::nvidia::NvidiaProcessorMetrics;
use nv_redfish::telemetry_service::MetricReport;
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

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_19_0.ComputerSystem";
const PROCESSOR_COLLECTION_DATA_TYPE: &str = "#ProcessorCollection.ProcessorCollection";
const PROCESSOR_DATA_TYPE: &str = "#Processor.v1_18_0.Processor";
const PROCESSOR_METRICS_DATA_TYPE: &str = "#ProcessorMetrics.v1_6_4.ProcessorMetrics";
const MEMORY_COLLECTION_DATA_TYPE: &str = "#MemoryCollection.MemoryCollection";
const MEMORY_DATA_TYPE: &str = "#Memory.v1_19_0.Memory";
const MEMORY_METRICS_DATA_TYPE: &str = "#MemoryMetrics.v1_7_1.MemoryMetrics";
const CHASSIS_COLLECTION_DATA_TYPE: &str = "#ChassisCollection.ChassisCollection";
const CHASSIS_DATA_TYPE: &str = "#Chassis.v1_22_0.Chassis";
const ENVIRONMENT_METRICS_DATA_TYPE: &str = "#EnvironmentMetrics.v1_3_2.EnvironmentMetrics";
const TELEMETRY_SERVICE_DATA_TYPE: &str = "#TelemetryService.v1_4_1.TelemetryService";
const METRIC_REPORT_COLLECTION_DATA_TYPE: &str = "#MetricReportCollection.MetricReportCollection";
const METRIC_REPORT_DATA_TYPE: &str = "#MetricReport.v1_5_2.MetricReport";

const GPU_METRICS_DATA_TYPE: &str = "#NvidiaProcessorMetrics.v1_5_0.NvidiaGPUProcessorMetrics";

#[test]
async fn processor_metrics_oem_nvidia_reads_gpu_shape() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let processor = get_processor(bmc.clone(), &ids).await?;

    bmc.expect(Expect::get(
        &ids.processor_metrics,
        json!({
            ODATA_ID: &ids.processor_metrics,
            ODATA_TYPE: PROCESSOR_METRICS_DATA_TYPE,
            "Id": "ProcessorMetrics",
            "Name": "Processor Metrics",
            "Oem": { "Nvidia": {
                ODATA_TYPE: GPU_METRICS_DATA_TYPE,
                "SMUtilizationPercent": 42.5,
                "ThrottleReasons": ["SWPowerCap"],
            }},
        }),
    ));

    let metrics = processor
        .metrics()
        .await?
        .expect("processor must expose metrics");
    let oem = metrics
        .oem_nvidia()?
        .expect("NVIDIA OEM extension must be available");

    let NvidiaProcessorMetrics::Gpu(gpu) = &oem else {
        panic!("payload declares the GPU shape");
    };
    assert_eq!(gpu.sm_utilization_percent.flatten(), Some(42.5));
    // Shared properties are reachable without matching the variant.
    assert_eq!(
        oem.common().throttle_reasons.clone().flatten(),
        Some(vec!["SWPowerCap".to_owned()])
    );

    Ok(())
}

#[test]
async fn processor_metrics_oem_nvidia_unknown_shape_reads_as_generic(
) -> Result<(), Box<dyn StdError>> {
    // `@odata.type` only selects between shapes. When it is absent --
    // or names a shape this version does not know -- the extension must
    // still be reported, with the shared properties populated. Dropping
    // it would discard every metric on the resource.
    for odata_type in [
        None,
        Some("#NvidiaProcessorMetrics.v9_9_0.NvidiaSomethingElseMetrics"),
    ] {
        let bmc = Arc::new(Bmc::default());
        let ids = ids();
        let processor = get_processor(bmc.clone(), &ids).await?;

        let mut nvidia = json!({ "ThrottleReasons": ["HWSlowdown"] });
        if let Some(t) = odata_type {
            nvidia[ODATA_TYPE] = json!(t);
        }
        bmc.expect(Expect::get(
            &ids.processor_metrics,
            json!({
                ODATA_ID: &ids.processor_metrics,
                ODATA_TYPE: PROCESSOR_METRICS_DATA_TYPE,
                "Id": "ProcessorMetrics",
                "Name": "Processor Metrics",
                "Oem": { "Nvidia": nvidia },
            }),
        ));

        let oem = processor
            .metrics()
            .await?
            .expect("processor must expose metrics")
            .oem_nvidia()?
            .expect("NVIDIA OEM extension must be available");

        assert!(
            matches!(oem, NvidiaProcessorMetrics::Generic(_)),
            "unknown @odata.type {:?} must read as the generic shape",
            odata_type
        );
        assert_eq!(
            oem.common().throttle_reasons.clone().flatten(),
            Some(vec!["HWSlowdown".to_owned()])
        );
    }

    Ok(())
}

#[test]
async fn processor_metrics_oem_nvidia_absent_or_null_reads_as_none() -> Result<(), Box<dyn StdError>>
{
    for oem in [None, Some(json!({ "Nvidia": Value::Null }))] {
        let bmc = Arc::new(Bmc::default());
        let ids = ids();
        let processor = get_processor(bmc.clone(), &ids).await?;

        let mut payload = json!({
            ODATA_ID: &ids.processor_metrics,
            ODATA_TYPE: PROCESSOR_METRICS_DATA_TYPE,
            "Id": "ProcessorMetrics",
            "Name": "Processor Metrics",
        });
        if let Some(oem) = &oem {
            payload["Oem"] = oem.clone();
        }
        bmc.expect(Expect::get(&ids.processor_metrics, payload));

        let metrics = processor
            .metrics()
            .await?
            .expect("processor must expose metrics");
        assert!(
            metrics.oem_nvidia()?.is_none(),
            "Oem {:?} must read as absent",
            oem
        );
    }

    Ok(())
}

#[test]
async fn processor_metrics_oem_nvidia_malformed_payload_is_parse_error(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let processor = get_processor(bmc.clone(), &ids).await?;

    bmc.expect(Expect::get(
        &ids.processor_metrics,
        json!({
            ODATA_ID: &ids.processor_metrics,
            ODATA_TYPE: PROCESSOR_METRICS_DATA_TYPE,
            "Id": "ProcessorMetrics",
            "Name": "Processor Metrics",
            "Oem": { "Nvidia": 42 },
        }),
    ));

    let metrics = processor
        .metrics()
        .await?
        .expect("processor must expose metrics");
    let err = match metrics.oem_nvidia() {
        Ok(v) => panic!("expected parse error, got Some: {}", v.is_some()),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("invalid type"),
        "unexpected error: {}",
        err
    );

    Ok(())
}

#[test]
async fn memory_metrics_oem_nvidia_reads_row_remapping() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let memory = get_memory(bmc.clone(), &ids).await?;

    bmc.expect(Expect::get(
        &ids.memory_metrics,
        json!({
            ODATA_ID: &ids.memory_metrics,
            ODATA_TYPE: MEMORY_METRICS_DATA_TYPE,
            "Id": "MemoryMetrics",
            "Name": "Memory Metrics",
            "Oem": { "Nvidia": {
                ODATA_TYPE: "#NvidiaMemoryMetrics.v1_2_0.NvidiaGPUMemoryMetrics",
                "RowRemapping": {
                    "CorrectableRowRemappingCount": 3,
                    "UncorrectableRowRemappingCount": 1,
                },
            }},
        }),
    ));

    let oem = memory
        .metrics()
        .await?
        .expect("memory must expose metrics")
        .oem_nvidia()?
        .expect("NVIDIA OEM extension must be available");
    let remapping = oem
        .row_remapping
        .as_ref()
        .expect("payload declares RowRemapping");
    assert_eq!(remapping.correctable_row_remapping_count.flatten(), Some(3));
    assert_eq!(
        remapping.uncorrectable_row_remapping_count.flatten(),
        Some(1)
    );

    Ok(())
}

#[test]
async fn memory_metrics_oem_nvidia_malformed_payload_is_parse_error(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let memory = get_memory(bmc.clone(), &ids).await?;

    bmc.expect(Expect::get(
        &ids.memory_metrics,
        json!({
            ODATA_ID: &ids.memory_metrics,
            ODATA_TYPE: MEMORY_METRICS_DATA_TYPE,
            "Id": "MemoryMetrics",
            "Name": "Memory Metrics",
            "Oem": { "Nvidia": "not an object" },
        }),
    ));

    let metrics = memory.metrics().await?.expect("memory must expose metrics");
    let err = match metrics.oem_nvidia() {
        Ok(v) => panic!("expected parse error, got Some: {}", v.is_some()),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("invalid type"),
        "unexpected error: {}",
        err
    );

    Ok(())
}

#[test]
async fn environment_metrics_oem_nvidia_reads_power_limits() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_chassis(bmc.clone(), &ids).await?;

    bmc.expect(Expect::get(
        &ids.environment_metrics,
        json!({
            ODATA_ID: &ids.environment_metrics,
            ODATA_TYPE: ENVIRONMENT_METRICS_DATA_TYPE,
            "Id": "EnvironmentMetrics",
            "Name": "Environment Metrics",
            "Oem": { "Nvidia": {
                ODATA_TYPE: "#NvidiaEnvironmentMetrics.v1_4_0.NvidiaEnvironmentMetrics",
                "PowerLimitPersistency": true,
                "GPUViewCPULimitWatts": 350.0,
            }},
        }),
    ));

    let oem = chassis
        .environment_metrics()
        .await?
        .expect("chassis must expose environment metrics")
        .oem_nvidia()?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(oem.power_limit_persistency.flatten(), Some(true));
    assert_eq!(oem.gpu_view_cpu_limit_watts.flatten(), Some(350.0));

    Ok(())
}

#[test]
async fn metric_report_oem_nvidia_reads_sensing_interval() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_service_root(bmc.clone(), &ids).await?;

    bmc.expect(Expect::get(
        &ids.telemetry_service,
        json!({
            ODATA_ID: &ids.telemetry_service,
            ODATA_TYPE: TELEMETRY_SERVICE_DATA_TYPE,
            "Id": "TelemetryService",
            "Name": "Telemetry Service",
            "MetricReports": { ODATA_ID: &ids.metric_reports },
        }),
    ));
    bmc.expect(Expect::get(
        &ids.metric_reports,
        json!({
            ODATA_ID: &ids.metric_reports,
            ODATA_TYPE: METRIC_REPORT_COLLECTION_DATA_TYPE,
            "Id": "MetricReports",
            "Name": "Metric Report Collection",
            "Members": [{ ODATA_ID: &ids.metric_report }],
            "Members@odata.count": 1,
        }),
    ));
    bmc.expect(Expect::get(
        &ids.metric_report,
        json!({
            ODATA_ID: &ids.metric_report,
            ODATA_TYPE: METRIC_REPORT_DATA_TYPE,
            "Id": "PlatformMetrics",
            "Name": "Platform Metrics",
            "Oem": { "Nvidia": {
                ODATA_TYPE: "#NvidiaMetricReport.v1_1_0.NvidiaMetricReport",
                "SensingIntervalMilliseconds": 1000,
                "MetricValueStale": false,
            }},
        }),
    ));

    let service = root
        .telemetry_service()
        .await?
        .expect("service root must expose a telemetry service");
    let links = service
        .metric_report_links()
        .await?
        .expect("telemetry service must expose metric reports");
    assert_eq!(links.len(), 1);
    let report: MetricReport<Bmc> = links[0].upgrade().await?;

    let oem = report
        .oem_nvidia()?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(oem.sensing_interval_milliseconds.flatten(), Some(1000));
    assert_eq!(oem.metric_value_stale.flatten(), Some(false));

    Ok(())
}

async fn get_processor(
    bmc: Arc<Bmc>,
    ids: &Ids,
) -> Result<nv_redfish::computer_system::Processor<Bmc>, Box<dyn StdError>> {
    let system = get_system(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.processors,
        json!({
            ODATA_ID: &ids.processors,
            ODATA_TYPE: PROCESSOR_COLLECTION_DATA_TYPE,
            "Id": "Processors",
            "Name": "Processor Collection",
            "Members": [{
                ODATA_ID: &ids.processor,
                ODATA_TYPE: PROCESSOR_DATA_TYPE,
                "Id": "GPU_0",
                "Name": "GPU 0",
                "Metrics": { ODATA_ID: &ids.processor_metrics },
            }],
        }),
    ));
    let processors = system
        .processors()
        .await?
        .expect("system must expose processors");
    assert_eq!(processors.len(), 1);
    Ok(processors
        .into_iter()
        .next()
        .expect("single processor must exist"))
}

async fn get_memory(
    bmc: Arc<Bmc>,
    ids: &Ids,
) -> Result<nv_redfish::computer_system::Memory<Bmc>, Box<dyn StdError>> {
    let system = get_system(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.memory,
        json!({
            ODATA_ID: &ids.memory,
            ODATA_TYPE: MEMORY_COLLECTION_DATA_TYPE,
            "Id": "Memory",
            "Name": "Memory Collection",
            "Members": [{
                ODATA_ID: &ids.memory_module,
                ODATA_TYPE: MEMORY_DATA_TYPE,
                "Id": "GPU_DRAM_0",
                "Name": "GPU DRAM 0",
                "Metrics": { ODATA_ID: &ids.memory_metrics },
            }],
        }),
    ));
    let modules = system
        .memory_modules()
        .await?
        .expect("system must expose memory");
    assert_eq!(modules.len(), 1);
    Ok(modules
        .into_iter()
        .next()
        .expect("single memory module must exist"))
}

async fn get_system(bmc: Arc<Bmc>, ids: &Ids) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.systems,
        json!({
            ODATA_ID: &ids.systems,
            ODATA_TYPE: SYSTEM_COLLECTION_DATA_TYPE,
            "Id": "Systems",
            "Name": "Computer System Collection",
            "Members": [{
                ODATA_ID: &ids.system,
                ODATA_TYPE: SYSTEM_DATA_TYPE,
                "Id": "HGX_Baseboard_0",
                "Name": "HGX Baseboard 0",
                "Processors": { ODATA_ID: &ids.processors },
                "Memory": { ODATA_ID: &ids.memory },
            }],
        }),
    ));
    let systems = root
        .systems()
        .await?
        .expect("service root must expose systems");
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single system must exist"))
}

async fn get_chassis(bmc: Arc<Bmc>, ids: &Ids) -> Result<Chassis<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.chassis_collection,
        json!({
            ODATA_ID: &ids.chassis_collection,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [{
                ODATA_ID: &ids.chassis,
                ODATA_TYPE: CHASSIS_DATA_TYPE,
                "Id": "HGX_Chassis_0",
                "Name": "HGX Chassis 0",
                "ChassisType": "RackMount",
                "EnvironmentMetrics": { ODATA_ID: &ids.environment_metrics },
            }],
        }),
    ));
    let collection = root
        .chassis()
        .await?
        .expect("service root must expose chassis");
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single chassis must exist"))
}

async fn expect_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root,
        json!({
            ODATA_ID: &ids.root,
            ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": { "ExpandQuery": { "NoLinks": true } },
            "Systems": { ODATA_ID: &ids.systems },
            "Chassis": { ODATA_ID: &ids.chassis_collection },
            "TelemetryService": { ODATA_ID: &ids.telemetry_service },
            "Links": {
                "Sessions": { ODATA_ID: format!("{}/SessionService/Sessions", ids.root) }
            },
        }),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct Ids {
    root: ODataId,
    systems: String,
    system: String,
    processors: String,
    processor: String,
    processor_metrics: String,
    memory: String,
    memory_module: String,
    memory_metrics: String,
    chassis_collection: String,
    chassis: String,
    environment_metrics: String,
    telemetry_service: String,
    metric_reports: String,
    metric_report: String,
}

fn ids() -> Ids {
    let root = ODataId::service_root();
    let systems = format!("{root}/Systems");
    let system = format!("{systems}/HGX_Baseboard_0");
    let processors = format!("{system}/Processors");
    let processor = format!("{processors}/GPU_0");
    let processor_metrics = format!("{processor}/ProcessorMetrics");
    let memory = format!("{system}/Memory");
    let memory_module = format!("{memory}/GPU_DRAM_0");
    let memory_metrics = format!("{memory_module}/MemoryMetrics");
    let chassis_collection = format!("{root}/Chassis");
    let chassis = format!("{chassis_collection}/HGX_Chassis_0");
    let environment_metrics = format!("{chassis}/EnvironmentMetrics");
    let telemetry_service = format!("{root}/TelemetryService");
    let metric_reports = format!("{telemetry_service}/MetricReports");
    let metric_report = format!("{metric_reports}/PlatformMetrics");
    Ids {
        root,
        systems,
        system,
        processors,
        processor,
        processor_metrics,
        memory,
        memory_module,
        memory_metrics,
        chassis_collection,
        chassis,
        environment_metrics,
        telemetry_service,
        metric_reports,
        metric_report,
    }
}
