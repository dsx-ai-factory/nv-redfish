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

//! Integration tests of typed asynchronous result polling.

use std::error::Error as StdError;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use nv_redfish::core::AsyncTask;
use nv_redfish::core::ModificationResponse;
use nv_redfish::core::ODataId;
use nv_redfish::task_service::TaskService;
use nv_redfish::task_service::TypedModificationResponse;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;

const TASK_SERVICE_PATH: &str = "/redfish/v1/TaskService";
const MONITOR_PATH: &str = "/redfish/v1/TaskService/Tasks/42/Monitor";
const RESULT_PATH: &str = "/redfish/v1/ComponentIntegrity/1/Actions/GetMeasurements/data";

async fn setup() -> Result<(Arc<Bmc>, TaskService<Bmc>), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    bmc.expect(Expect::get(
        "/redfish/v1",
        json!({
            ODATA_ID: "/redfish/v1",
            ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
            "Id": "RootService",
            "Name": "Root Service",
            "Tasks": {
                ODATA_ID: TASK_SERVICE_PATH
            },
            "Links": {
                "Sessions": {
                    ODATA_ID: "/redfish/v1/SessionService/Sessions"
                }
            }
        }),
    ));
    bmc.expect(Expect::get(
        TASK_SERVICE_PATH,
        json!({
            ODATA_ID: TASK_SERVICE_PATH,
            ODATA_TYPE: "#TaskService.v1_1_4.TaskService",
            "Id": "TaskService",
            "Name": "Task Service",
            "Tasks": {
                ODATA_ID: "/redfish/v1/TaskService/Tasks"
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;
    let task_service = root
        .task_service()
        .await?
        .ok_or_else(|| IoError::new(ErrorKind::NotFound, "expected task service"))?;
    Ok((bmc, task_service))
}

fn pending() -> ModificationResponse<Value> {
    ModificationResponse::Task(AsyncTask {
        location: ODataId::from(MONITOR_PATH.to_string()).into(),
        retry_after: Some(Duration::from_secs(30)),
    })
}

fn result_path() -> ODataId {
    ODataId::from(RESULT_PATH.to_string())
}

#[test]
async fn poll_returns_immediate_entity_once() -> Result<(), Box<dyn StdError>> {
    let (_bmc, task_service) = setup().await?;
    let mut response =
        TypedModificationResponse::from_typed_action(ModificationResponse::Entity(json!({
            "result": "immediate"
        })));

    assert_eq!(
        response.poll(&task_service).await?,
        Some(json!({"result": "immediate"}))
    );
    assert!(matches!(
        response.poll(&task_service).await,
        Err(Error::TaskAlreadyFinished)
    ));

    Ok(())
}

#[test]
async fn poll_immediate_empty_reads_compatibility_result() -> Result<(), Box<dyn StdError>> {
    let (bmc, task_service) = setup().await?;
    bmc.expect(Expect::operation_response_result(
        RESULT_PATH,
        json!({"result": "compat"}),
    ));
    let mut response = TypedModificationResponse::<Value>::with_compatibility_result(
        ModificationResponse::Empty,
        result_path(),
    );

    assert_eq!(
        response.poll(&task_service).await?,
        Some(json!({"result": "compat"}))
    );

    Ok(())
}

#[test]
async fn poll_replaces_retry_after_until_monitor_returns_result() -> Result<(), Box<dyn StdError>> {
    let (bmc, task_service) = setup().await?;
    bmc.expect(Expect::operation_response_pending(
        MONITOR_PATH,
        None,
        Some(Duration::from_secs(9)),
    ));
    bmc.expect(Expect::operation_response_pending(MONITOR_PATH, None, None));
    bmc.expect(Expect::operation_response_result(
        MONITOR_PATH,
        json!({"result": "monitor"}),
    ));
    let mut response = TypedModificationResponse::from_typed_action(pending());
    assert_eq!(response.retry_after(), Some(Duration::from_secs(30)));

    assert_eq!(response.poll(&task_service).await?, None);
    assert_eq!(response.retry_after(), Some(Duration::from_secs(9)));
    assert_eq!(response.poll(&task_service).await?, None);
    assert_eq!(response.retry_after(), None);
    assert_eq!(
        response.poll(&task_service).await?,
        Some(json!({"result": "monitor"}))
    );

    Ok(())
}

#[test]
async fn poll_retries_same_step_after_request_error() -> Result<(), Box<dyn StdError>> {
    let (bmc, task_service) = setup().await?;
    bmc.expect(Expect::operation_response_status(MONITOR_PATH, 503));
    bmc.expect(Expect::operation_response_result(
        MONITOR_PATH,
        json!({"result": "monitor"}),
    ));
    let mut response = TypedModificationResponse::from_typed_action(pending());

    assert!(matches!(
        response.poll(&task_service).await,
        Err(Error::Bmc(_))
    ));
    assert_eq!(
        response.poll(&task_service).await?,
        Some(json!({"result": "monitor"}))
    );

    Ok(())
}

#[test]
async fn poll_empty_monitor_without_result_source_is_unavailable() -> Result<(), Box<dyn StdError>>
{
    let (bmc, task_service) = setup().await?;
    bmc.expect(Expect::operation_response_empty(MONITOR_PATH));
    let mut response = TypedModificationResponse::from_typed_action(pending());

    assert!(matches!(
        response.poll(&task_service).await,
        Err(Error::TaskResultUnavailable)
    ));

    Ok(())
}

#[test]
async fn poll_empty_monitor_reads_compatibility_result() -> Result<(), Box<dyn StdError>> {
    let (bmc, task_service) = setup().await?;
    bmc.expect(Expect::operation_response_empty(MONITOR_PATH));
    bmc.expect(Expect::operation_response_status(RESULT_PATH, 503));
    bmc.expect(Expect::operation_response_result(
        RESULT_PATH,
        json!({"result": "compat"}),
    ));
    let mut response =
        TypedModificationResponse::with_compatibility_result(pending(), result_path());

    assert!(matches!(
        response.poll(&task_service).await,
        Err(Error::Bmc(_))
    ));
    assert_eq!(
        response.poll(&task_service).await?,
        Some(json!({"result": "compat"}))
    );

    Ok(())
}
