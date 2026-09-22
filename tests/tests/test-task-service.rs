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

//! Integration tests of Task Service.

use std::error::Error as StdError;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::sync::Arc;

use nv_redfish::core::AsyncTask;
use nv_redfish::core::ODataId;
use nv_redfish::schema::resource::Health as TaskStatus;
use nv_redfish::schema::task::TaskState;
use nv_redfish::task_service::task_payload_location;
use nv_redfish::ServiceRoot;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use tokio::test;

const TASK_SERVICE_PATH: &str = "/redfish/v1/TaskService";
const TASK_PATH: &str = "/redfish/v1/TaskService/Tasks/42";

#[test]
async fn task_link_fetch_exposes_schema_fields() -> Result<(), Box<dyn StdError>> {
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

    bmc.expect(Expect::get(
        TASK_PATH,
        json!({
            ODATA_ID: TASK_PATH,
            ODATA_TYPE: "#Task.v1_4_3.Task",
            "Id": "42",
            "Name": "Task 42",
            "TaskState": "Running",
            "TaskStatus": "OK",
            "PercentComplete": 55,
            "Payload": {
                "HttpHeaders": [
                    "Content-Type: application/json",
                    "Location: /redfish/v1/ComponentIntegrity/1/Actions/GetMeasurements/Data"
                ]
            },
            "Messages": [{
                "MessageId": "Base.1.0.TaskMessage",
                "Message": "Task message."
            }]
        }),
    ));

    let root = ServiceRoot::new(bmc).await?;
    let task_service = root
        .task_service()
        .await?
        .ok_or_else(|| IoError::new(ErrorKind::NotFound, "expected task service"))?;

    let invalid_task = AsyncTask {
        location: ODataId::from(
            "/redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/Jobs/1".to_string(),
        )
        .into(),
        task_resource: None,
        retry_after: None,
    };

    let Err(error) = task_service.task_link(&invalid_task) else {
        return Err(String::from("expected invalid task location").into());
    };

    assert_eq!(
        error.to_string(),
        "Task location /redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/Jobs/1 is not in TaskService Tasks collection /redfish/v1/TaskService/Tasks"
    );

    let collection_task = AsyncTask {
        location: ODataId::from("/redfish/v1/TaskService/Tasks".to_string()).into(),
        task_resource: None,
        retry_after: None,
    };

    let Err(error) = task_service.task_link(&collection_task) else {
        return Err(String::from("expected collection location to be invalid").into());
    };

    assert_eq!(
        error.to_string(),
        "Task location /redfish/v1/TaskService/Tasks is not in TaskService Tasks collection /redfish/v1/TaskService/Tasks"
    );

    let async_task = AsyncTask {
        location: ODataId::from("/redfish/v1/TaskService/TaskMonitors/task-1".to_string()).into(),
        task_resource: Some(ODataId::from(TASK_PATH.to_string())),
        retry_after: None,
    };

    let task_link = task_service.task_link(&async_task)?;
    assert_eq!(task_link.odata_id().to_string(), TASK_PATH);

    let task = task_link.fetch().await?;

    assert_eq!(task.task_state, Some(TaskState::Running));
    assert_eq!(task.task_status, Some(TaskStatus::Ok));
    assert_eq!(task.percent_complete.flatten(), Some(55));
    assert_eq!(
        task_payload_location(&task).map(|location| location.to_string()),
        Some("/redfish/v1/ComponentIntegrity/1/Actions/GetMeasurements/Data".to_string())
    );

    let messages = task
        .messages
        .iter()
        .flatten()
        .filter_map(|message| message.message.as_deref())
        .collect::<Vec<_>>();

    assert_eq!(messages, vec!["Task message."]);

    Ok(())
}
