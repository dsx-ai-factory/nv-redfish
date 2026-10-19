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

//! Integration tests of asynchronous action result polling.

use std::error::Error as StdError;
use std::task::Poll;
use std::time::Duration;

use nv_redfish::core::action::ActionTarget;
use nv_redfish::core::Action;
use nv_redfish::core::AsyncTask;
use nv_redfish::core::ODataId;
use nv_redfish::schema::task::TaskState;
use nv_redfish::task_service::AsyncActionResult;
use nv_redfish::Error;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;
use tokio::test;

const ACTION: &str =
    "/redfish/v1/ComponentIntegrity/EROT_BIOS_0/Actions/ComponentIntegrity.SPDMGetSignedMeasurements";
const MONITOR: &str = "/redfish/v1/TaskService/Tasks/42/Monitor";
const TASK: &str = "/redfish/v1/TaskService/Tasks/95";
const RESULT: &str =
    "/redfish/v1/ComponentIntegrity/EROT_BIOS_0/Actions/SPDMGetSignedMeasurements/Data";

#[derive(Debug, PartialEq, Deserialize)]
struct Evidence {
    #[serde(rename = "SignedMeasurements")]
    signed_measurements: String,
}

fn action<R>() -> Action<Value, R> {
    Action::new(ActionTarget::new(ACTION.to_string()))
}

fn params() -> Value {
    json!({"Nonce": "00"})
}

fn evidence(signed_measurements: &str) -> Value {
    json!({"SignedMeasurements": signed_measurements})
}

fn expected(signed_measurements: &str) -> Option<Evidence> {
    Some(Evidence {
        signed_measurements: signed_measurements.to_string(),
    })
}

fn monitor_task(retry_after: Option<Duration>) -> AsyncTask {
    AsyncTask {
        location: ODataId::from(MONITOR.to_string()).into(),
        retry_after,
        task: None,
    }
}

/// Task resource as AMI Viking reports it while polled at the Task.
fn viking_task(state: &str, fields: Value) -> Value {
    nv_redfish_tests::json_merge([
        &json!({
            "@odata.id": TASK,
            "@odata.type": "#Task.v1_4_2.Task",
            "Id": "95",
            "Name": "Redfish UpdateManager OverIPC Task-3220",
            "TaskState": state
        }),
        &fields,
    ])
}

async fn start_at_monitor(bmc: &Bmc) -> Result<AsyncActionResult<Evidence>, Box<dyn StdError>> {
    bmc.expect(Expect::action_task(
        ACTION,
        params(),
        monitor_task(Some(Duration::from_secs(30))),
    ));
    Ok(AsyncActionResult::start(bmc, &action(), &params()).await?)
}

#[test]
async fn immediate_result_is_returned_once() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    bmc.expect(Expect::action(ACTION, params(), evidence("immediate")));
    let mut result = AsyncActionResult::start(&bmc, &action(), &params()).await?;

    assert_eq!(result.poll_result(&bmc).await?, expected("immediate"));
    assert!(matches!(
        result.poll_result(&bmc).await,
        Err(Error::TaskAlreadyFinished)
    ));
    Ok(())
}

#[test]
async fn monitor_is_polled_until_result() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    let mut result = start_at_monitor(&bmc).await?;
    assert_eq!(result.retry_after(), Some(Duration::from_secs(30)));

    bmc.expect(Expect::poll_pending(
        MONITOR,
        None,
        Some(Duration::from_secs(9)),
    ));
    assert_eq!(result.poll_result(&bmc).await?, None);
    assert_eq!(result.retry_after(), Some(Duration::from_secs(9)));

    bmc.expect(Expect::poll_result(MONITOR, evidence("monitor")));
    assert_eq!(result.poll_result(&bmc).await?, expected("monitor"));
    Ok(())
}

#[test]
async fn request_error_retries_same_step() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    let mut result = start_at_monitor(&bmc).await?;

    bmc.expect(Expect::poll_status(MONITOR, 503));
    assert!(matches!(result.poll_result(&bmc).await, Err(Error::Bmc(_))));

    bmc.expect(Expect::poll_result(MONITOR, evidence("monitor")));
    assert_eq!(result.poll_result(&bmc).await?, expected("monitor"));
    Ok(())
}

#[test]
async fn cancelled_poll_keeps_step() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    let mut result = start_at_monitor(&bmc).await?;

    bmc.expect(Expect::poll_wait(MONITOR));
    let mut poll = Box::pin(result.poll_result(&bmc));
    assert!(matches!(futures_util::poll!(poll.as_mut()), Poll::Pending));
    drop(poll);

    bmc.expect(Expect::poll_result(MONITOR, evidence("monitor")));
    assert_eq!(result.poll_result(&bmc).await?, expected("monitor"));
    Ok(())
}

#[test]
async fn finished_location_is_read_in_same_poll() -> Result<(), Box<dyn StdError>> {
    // A monitor answering `204` with `Location`, as on Wiwynn.
    let bmc = Bmc::default();
    let mut result = start_at_monitor(&bmc).await?;

    bmc.expect(Expect::poll_result(MONITOR, json!({"@odata.id": RESULT})));
    bmc.expect(Expect::poll_result(RESULT, evidence("wiwynn")));
    assert_eq!(result.poll_result(&bmc).await?, expected("wiwynn"));
    Ok(())
}

#[test]
async fn task_body_is_polled_until_recorded_location() -> Result<(), Box<dyn StdError>> {
    // The action answers `200` with a Task body, as on AMI Viking.
    let bmc = Bmc::default();
    bmc.expect(Expect::action(
        ACTION,
        params(),
        viking_task("Running", json!({})),
    ));
    let mut result = AsyncActionResult::<Evidence>::start(&bmc, &action(), &params()).await?;

    bmc.expect(Expect::poll_result(TASK, viking_task("Running", json!({}))));
    assert_eq!(result.poll_result(&bmc).await?, None);

    bmc.expect(Expect::poll_result(
        TASK,
        viking_task(
            "Completed",
            json!({
                "Messages": [{
                    "@odata.type": "#Message.v1_0_8.Message",
                    "Message": "The task with id 95 has completed.",
                    "MessageArgs": ["95"],
                    "MessageId": "TaskEvent.1.0.TaskCompletedOK",
                    "Resolution": "None.",
                    "Severity": "OK"
                }],
                "Payload": { "HttpHeaders": [format!("Location: {RESULT}")] },
                "PercentComplete": 100,
                "TaskStatus": "OK"
            }),
        ),
    ));
    bmc.expect(Expect::poll_result(RESULT, evidence("viking")));
    assert_eq!(result.poll_result(&bmc).await?, expected("viking"));
    Ok(())
}

#[test]
async fn moved_monitor_is_resumed() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    let moved = "/redfish/v1/TaskService/Tasks/42/Monitor?page=2";
    let mut result = start_at_monitor(&bmc).await?;

    bmc.expect(Expect::poll_pending(MONITOR, Some(moved), None));
    assert_eq!(result.poll_result(&bmc).await?, None);
    let saved = result
        .pending_task()
        .cloned()
        .ok_or("expected a pending task")?;
    assert_eq!(saved.location.0.to_string(), moved);

    bmc.expect(Expect::poll_result(moved, evidence("resumed")));
    let mut resumed = AsyncActionResult::resume(&action(), saved);
    assert_eq!(resumed.poll_result(&bmc).await?, expected("resumed"));
    Ok(())
}

#[test]
async fn finished_without_result_is_unavailable() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    let mut result = start_at_monitor(&bmc).await?;
    bmc.expect(Expect::poll_empty(MONITOR));
    assert!(matches!(
        result.poll_result(&bmc).await,
        Err(Error::TaskResultUnavailable)
    ));

    let mut result = AsyncActionResult::<Evidence>::resume(
        &action(),
        AsyncTask {
            location: ODataId::from(TASK.to_string()).into(),
            retry_after: None,
            task: None,
        },
    );
    bmc.expect(Expect::poll_result(
        TASK,
        viking_task("Completed", json!({})),
    ));
    assert!(matches!(
        result.poll_result(&bmc).await,
        Err(Error::TaskResultUnavailable)
    ));
    Ok(())
}

#[test]
async fn failed_task_reports_state_and_messages() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    let mut result = start_at_monitor(&bmc).await?;
    bmc.expect(Expect::poll_result(
        MONITOR,
        viking_task(
            "Exception",
            json!({"Messages": [{"MessageId": "SPDM.1.0.Timeout", "Message": "Responder timed out"}]}),
        ),
    ));

    let Err(Error::TaskFailed { state, messages }) = result.poll_result(&bmc).await else {
        return Err("expected a failed Task".into());
    };
    assert_eq!(state, TaskState::Exception);
    assert_eq!(messages[0].message.as_deref(), Some("Responder timed out"));
    assert!(matches!(
        result.poll_result(&bmc).await,
        Err(Error::TaskAlreadyFinished)
    ));
    Ok(())
}

#[test]
async fn action_without_return_value_completes() -> Result<(), Box<dyn StdError>> {
    let bmc = Bmc::default();
    bmc.expect(Expect::action_task(ACTION, params(), monitor_task(None)));
    let mut result = AsyncActionResult::<()>::start(&bmc, &action(), &params()).await?;

    bmc.expect(Expect::poll_empty(MONITOR));
    assert_eq!(result.poll_result(&bmc).await?, Some(()));
    Ok(())
}

#[test]
async fn action_without_return_value_ignores_created_location() -> Result<(), Box<dyn StdError>> {
    // `201 Created` with `Location` and no body names a created resource,
    // which is not a result an action without a return value can have.
    let bmc = Bmc::default();
    bmc.expect(Expect::action(
        ACTION,
        params(),
        json!({"@odata.id": "/redfish/v1/Systems/1/LogServices/Diag/Entries/7"}),
    ));
    let mut result = AsyncActionResult::<()>::start(&bmc, &action(), &params()).await?;

    assert!(result.pending_task().is_none());
    assert_eq!(result.poll_result(&bmc).await?, Some(()));
    Ok(())
}
