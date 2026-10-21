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

//! Integration tests of Session Service.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::session_service::SessionCollection;
use nv_redfish::session_service::SessionCreate;
use nv_redfish::session_service::SessionService;
use nv_redfish::session_service::SessionTypes;
use nv_redfish::ServiceRoot;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ODataId;
use nv_redfish_tests::assert_empty;
use nv_redfish_tests::assert_task;
use nv_redfish_tests::async_task;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;

use serde_json::json;
use tokio::test;

const ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const SESSION_SERVICE_DATA_TYPE: &str = "#SessionService.v1_1_5.SessionService";
const SESSIONS_DATA_TYPE: &str = "#SessionCollection.SessionCollection";
const SESSION_DATA_TYPE: &str = "#Session.v1_5_0.Session";

#[test]
async fn list_sessions() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let session_service = get_session_service(bmc.clone(), &root_id).await?;
    let session_id = format!(
        "{}/Sessions/1234567890ABCDEF",
        session_service.raw().odata_id()
    );
    let sessions = get_session_collection(
        bmc.clone(),
        &session_service,
        json!([{
            ODATA_ID: session_id,
            ODATA_TYPE: SESSION_DATA_TYPE,
            "Id": "1234567890ABCDEF",
            "Name": "User Session",
            "UserName": "Administrator",
            "SessionType": "ManagerConsole"
        }]),
    )
    .await?;

    let sessions = sessions.members().await?;
    assert_eq!(sessions.len(), 1);
    let session = sessions.first().unwrap().raw();
    assert_eq!(session.user_name, Some(Some("Administrator".into())));
    assert_eq!(
        session.session_type,
        Some(Some(SessionTypes::ManagerConsole))
    );
    Ok(())
}

#[test]
async fn create_session() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let session_service = get_session_service(bmc.clone(), &root_id).await?;
    let sessions = get_session_collection(bmc.clone(), &session_service, json!([])).await?;
    let sessions_id = format!("{}/Sessions", session_service.raw().odata_id());
    let session_id = format!("{sessions_id}/1234567890ABCDEF");
    let create = SessionCreate::builder("Administrator".into(), "password".into()).build();

    bmc.expect(Expect::create_session(
        &sessions_id,
        serde_json::to_value(&create)?,
        json!({
            ODATA_ID: &session_id,
            ODATA_TYPE: SESSION_DATA_TYPE,
            "ClientOriginIPAddress": "127.0.0.1",
            "CreatedTime": "2026-03-18T00:47:59-05:00",
            "Description": "User Session",
            "Id": "1234567890ABCDEF",
            "Name": "User Session",
            "UserName": "Administrator",
            "SessionType": "ManagerConsole"
        }),
        "session-token-123",
        &session_id,
    ));

    let session = sessions.create_session(&create).await?;
    assert_eq!(session.auth_token(), Some("session-token-123"));
    assert_eq!(
        session.location().map(ToString::to_string),
        Some(session_id)
    );
    assert_eq!(session.raw().user_name, Some(Some("Administrator".into())));
    assert_eq!(
        session.raw().client_origin_ip_address,
        Some(Some("127.0.0.1".into()))
    );
    assert_eq!(
        session.raw().session_type,
        Some(Some(SessionTypes::ManagerConsole))
    );
    Ok(())
}

#[test]
async fn delete_created_session_uses_location() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let session_service = get_session_service(bmc.clone(), &root_id).await?;
    let sessions = get_session_collection(bmc.clone(), &session_service, json!([])).await?;
    let sessions_id = format!("{}/Sessions", session_service.raw().odata_id());
    let body_session_id = format!("{sessions_id}/body-id");
    let location_session_id = format!("{sessions_id}/location-id");
    let create = SessionCreate::builder("Administrator".into(), "password".into()).build();

    bmc.expect(Expect::create_session(
        &sessions_id,
        serde_json::to_value(&create)?,
        json!({
            ODATA_ID: &body_session_id,
            ODATA_TYPE: SESSION_DATA_TYPE,
            "Id": "body-id",
            "Name": "User Session",
            "UserName": "Administrator",
            "SessionType": "ManagerConsole"
        }),
        "session-token-123",
        &location_session_id,
    ));

    let session = sessions.create_session(&create).await?;
    bmc.expect(Expect::delete(&location_session_id));
    assert_empty(session.delete().await?);

    Ok(())
}

#[test]
async fn delete_session_preserves_async_task() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let session_service = get_session_service(bmc.clone(), &root_id).await?;
    let session_id = format!(
        "{}/Sessions/1234567890ABCDEF",
        session_service.raw().odata_id()
    );
    let sessions = get_session_collection(
        bmc.clone(),
        &session_service,
        json!([{
            ODATA_ID: &session_id,
            ODATA_TYPE: SESSION_DATA_TYPE,
            "Id": "1234567890ABCDEF",
            "Name": "User Session",
            "UserName": "Administrator",
            "SessionType": "ManagerConsole"
        }]),
    )
    .await?;

    let session = sessions.members().await?.into_iter().next().unwrap();
    let task_id = "/redfish/v1/TaskService/Tasks/51";

    bmc.expect(Expect::delete_task(&session_id, async_task(task_id, 6)));

    assert_task(session.delete().await?, task_id, 6);

    Ok(())
}

async fn get_session_service(
    bmc: Arc<Bmc>,
    root_id: &ODataId,
) -> Result<SessionService<Bmc>, Box<dyn StdError>> {
    let session_service_id = format!("{root_id}/SessionService");
    let sessions_id = format!("{session_service_id}/Sessions");
    bmc.expect(Expect::get(
        root_id,
        json!({
            ODATA_ID: root_id,
            ODATA_TYPE: ROOT_DATA_TYPE,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "SessionService": {
                ODATA_ID: &session_service_id,
            },
            "Links": {
                "Sessions": {
                    ODATA_ID: &sessions_id,
                }
            },
        }),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &session_service_id,
        json!({
            ODATA_ID: &session_service_id,
            ODATA_TYPE: SESSION_SERVICE_DATA_TYPE,
            "Id": "SessionService",
            "Name": "Session Service",
            "ServiceEnabled": true,
            "SessionTimeout": 600,
            "Sessions": {
                ODATA_ID: &sessions_id,
            },
        }),
    ));
    Ok(service_root.session_service().await?.unwrap())
}

async fn get_session_collection(
    bmc: Arc<Bmc>,
    session_service: &SessionService<Bmc>,
    members: serde_json::Value,
) -> Result<SessionCollection<Bmc>, Box<dyn StdError>> {
    let sessions_id = format!("{}/Sessions", session_service.raw().odata_id());
    bmc.expect(Expect::expand(
        &sessions_id,
        json!({
            ODATA_ID: &sessions_id,
            ODATA_TYPE: SESSIONS_DATA_TYPE,
            "Name": "User Sessions",
            "Members": members,
        }),
    ));
    Ok(session_service.sessions().await?.unwrap())
}
