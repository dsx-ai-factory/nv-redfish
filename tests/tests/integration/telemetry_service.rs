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

//! Integration tests for Telemetry Service resources.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::telemetry_service::MetricDefinition;
use nv_redfish::telemetry_service::MetricDefinitionCreate;
use nv_redfish::telemetry_service::MetricDefinitionUpdate;
use nv_redfish::telemetry_service::MetricReportDefinition;
use nv_redfish::telemetry_service::MetricReportDefinitionCreate;
use nv_redfish::telemetry_service::MetricReportDefinitionUpdate;
use nv_redfish::telemetry_service::TelemetryService;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::assert_empty;
use nv_redfish_tests::assert_task;
use nv_redfish_tests::async_task;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;

use serde_json::json;
use serde_json::Value;
use tokio::test;

const ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const TELEMETRY_SERVICE_DATA_TYPE: &str = "#TelemetryService.v1_4_1.TelemetryService";
const METRIC_DEFINITION_COLLECTION_DATA_TYPE: &str =
    "#MetricDefinitionCollection.MetricDefinitionCollection";

const METRIC_DEFINITION_DATA_TYPE: &str = "#MetricDefinition.v1_3_5.MetricDefinition";
const METRIC_REPORT_DEFINITION_COLLECTION_DATA_TYPE: &str =
    "#MetricReportDefinitionCollection.MetricReportDefinitionCollection";

const METRIC_REPORT_DEFINITION_DATA_TYPE: &str =
    "#MetricReportDefinition.v1_4_7.MetricReportDefinition";

struct TelemetryIds {
    root: ODataId,
    service: String,
    metric_definitions: String,
    metric_definition: String,
    metric_report_definitions: String,
    metric_report_definition: String,
}

fn telemetry_ids() -> TelemetryIds {
    let root = ODataId::service_root();
    let service = format!("{root}/TelemetryService");
    let metric_definitions = format!("{service}/MetricDefinitions");
    let metric_definition = format!("{metric_definitions}/Temperature");
    let metric_report_definitions = format!("{service}/MetricReportDefinitions");
    let metric_report_definition = format!("{metric_report_definitions}/ThermalReport");

    TelemetryIds {
        root,
        service,
        metric_definitions,
        metric_definition,
        metric_report_definitions,
        metric_report_definition,
    }
}

fn single_member_collection(
    collection_id: &str,
    collection_type: &str,
    member_id: &str,
    member_type: &str,
    member_name: &str,
) -> Value {
    json!({
        ODATA_ID: collection_id,
        ODATA_TYPE: collection_type,
        "Name": member_name,
        "Members": [{
            ODATA_ID: member_id,
            ODATA_TYPE: member_type,
            "Id": member_name,
            "Name": member_name,
        }]
    })
}

fn service_root_payload(ids: &TelemetryIds) -> Value {
    json!({
        ODATA_ID: &ids.root,
        ODATA_TYPE: ROOT_DATA_TYPE,
        "Id": "RootService",
        "Name": "Root Service",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": true
            }
        },
        "TelemetryService": {
            ODATA_ID: &ids.service
        },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{}/SessionService/Sessions", ids.root)
            }
        }
    })
}

#[test]
async fn set_enabled_preserves_task_and_empty_responses() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = telemetry_ids();
    let service = get_telemetry_service(bmc.clone(), &ids).await?;
    let task_id = "/redfish/v1/TaskService/Tasks/61";

    bmc.expect(Expect::update_task(
        &ids.service,
        json!({ "ServiceEnabled": false }),
        async_task(task_id, 4),
    ));

    assert_task(service.set_enabled(false).await?, task_id, 4);

    bmc.expect(Expect::update_empty(
        &ids.service,
        json!({ "ServiceEnabled": true }),
    ));

    assert_empty(service.set_enabled(true).await?);

    Ok(())
}

#[test]
async fn create_definitions_preserves_task_and_empty_responses() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = telemetry_ids();
    let service = get_telemetry_service(bmc.clone(), &ids).await?;
    let metric_create = MetricDefinitionCreate::builder().build();
    let metric_report_create = MetricReportDefinitionCreate::builder().build();
    let metric_task_id = "/redfish/v1/TaskService/Tasks/62";

    bmc.expect(Expect::create_task(
        &ids.metric_definitions,
        json!({}),
        async_task(metric_task_id, 5),
    ));

    assert_task(
        service.create_metric_definition(&metric_create).await?,
        metric_task_id,
        5,
    );

    bmc.expect(Expect::create_empty(
        &ids.metric_report_definitions,
        json!({}),
    ));

    assert_empty(
        service
            .create_metric_report_definition(&metric_report_create)
            .await?,
    );

    Ok(())
}

#[test]
async fn update_and_delete_definitions_preserve_task_and_empty_responses(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = telemetry_ids();
    let service = get_telemetry_service(bmc.clone(), &ids).await?;
    let metric_definition = get_metric_definition(&bmc, &service, &ids).await?;
    let metric_report_definition = get_metric_report_definition(&bmc, &service, &ids).await?;
    let metric_update = MetricDefinitionUpdate::builder().build();
    let metric_report_update = MetricReportDefinitionUpdate::builder().build();
    let metric_update_task_id = "/redfish/v1/TaskService/Tasks/64";

    bmc.expect(Expect::update_task(
        &ids.metric_definition,
        json!({}),
        async_task(metric_update_task_id, 7),
    ));

    assert_task(
        metric_definition.update(&metric_update).await?,
        metric_update_task_id,
        7,
    );

    bmc.expect(Expect::delete(&ids.metric_definition));
    assert_empty(metric_definition.delete().await?);

    let metric_report_update_task_id = "/redfish/v1/TaskService/Tasks/66";

    bmc.expect(Expect::update_task(
        &ids.metric_report_definition,
        json!({}),
        async_task(metric_report_update_task_id, 9),
    ));

    assert_task(
        metric_report_definition
            .update(&metric_report_update)
            .await?,
        metric_report_update_task_id,
        9,
    );

    bmc.expect(Expect::update_empty(
        &ids.metric_report_definition,
        json!({}),
    ));

    assert_empty(
        metric_report_definition
            .update(&metric_report_update)
            .await?,
    );

    bmc.expect(Expect::delete(&ids.metric_report_definition));
    assert_empty(metric_report_definition.delete().await?);

    Ok(())
}

async fn get_telemetry_service(
    bmc: Arc<Bmc>,
    ids: &TelemetryIds,
) -> Result<TelemetryService<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(&ids.root, service_root_payload(ids)));

    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &ids.service,
        json!({
            ODATA_ID: &ids.service,
            ODATA_TYPE: TELEMETRY_SERVICE_DATA_TYPE,
            "Id": "TelemetryService",
            "Name": "Telemetry Service",
            "ServiceEnabled": true,
            "MetricDefinitions": {
                ODATA_ID: &ids.metric_definitions
            },
            "MetricReportDefinitions": {
                ODATA_ID: &ids.metric_report_definitions
            }
        }),
    ));

    root.telemetry_service()
        .await?
        .ok_or_else(|| std::io::Error::other("missing telemetry service").into())
}

async fn get_metric_definition(
    bmc: &Arc<Bmc>,
    service: &TelemetryService<Bmc>,
    ids: &TelemetryIds,
) -> Result<MetricDefinition<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::expand(
        &ids.metric_definitions,
        single_member_collection(
            &ids.metric_definitions,
            METRIC_DEFINITION_COLLECTION_DATA_TYPE,
            &ids.metric_definition,
            METRIC_DEFINITION_DATA_TYPE,
            "Temperature",
        ),
    ));

    service
        .metric_definitions()
        .await?
        .and_then(|mut definitions| definitions.pop())
        .ok_or_else(|| std::io::Error::other("missing metric definition").into())
}

async fn get_metric_report_definition(
    bmc: &Arc<Bmc>,
    service: &TelemetryService<Bmc>,
    ids: &TelemetryIds,
) -> Result<MetricReportDefinition<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::expand(
        &ids.metric_report_definitions,
        single_member_collection(
            &ids.metric_report_definitions,
            METRIC_REPORT_DEFINITION_COLLECTION_DATA_TYPE,
            &ids.metric_report_definition,
            METRIC_REPORT_DEFINITION_DATA_TYPE,
            "ThermalReport",
        ),
    ));

    service
        .metric_report_definitions()
        .await?
        .and_then(|mut definitions| definitions.pop())
        .ok_or_else(|| std::io::Error::other("missing metric report definition").into())
}
