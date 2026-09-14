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
//! An EventService stream resumed by event id: the id the request resumes
//! after and the id each event carries travel through the wrapper unchanged,
//! and the plain stream stays what it was.

use std::error::Error as StdError;
use std::sync::Arc;

use futures_util::TryStreamExt as _;
use nv_redfish::event_service::EventService;
use nv_redfish::event_service::EventStreamPayload;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const EVENT_SERVICE_DATA_TYPE: &str = "#EventService.v1_9_3.EventService";
const EVENT_DATA_TYPE: &str = "#Event.v1_9_2.Event";
const EVENT_SERVICE_ID: &str = "/redfish/v1/EventService";
const SSE_URI: &str = "/redfish/v1/EventService/SSE";

#[test]
async fn a_resumed_event_stream_carries_each_events_id() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let event_service = open_event_service(&bmc).await?;

    // Asked for the events after 41, the device answers event 42; the mock
    // holds the request to that id.
    bmc.expect(Expect::stream_events(
        SSE_URI,
        Some("41"),
        [(Some("42"), event(42))],
    ));
    let mut events = event_service.events_from(Some("41")).await?;
    let resumed = events.try_next().await?.expect("one event");
    assert_eq!(resumed.last_event_id.as_deref(), Some("42"));
    assert!(matches!(resumed.data, EventStreamPayload::Event(_)));
    assert!(events.try_next().await?.is_none());

    // The plain stream opens without an id and yields the payloads alone.
    bmc.expect(Expect::stream(SSE_URI, json!([event(43)])));
    let mut payloads = event_service.events().await?;
    assert!(matches!(
        payloads.try_next().await?,
        Some(EventStreamPayload::Event(_))
    ));
    assert!(payloads.try_next().await?.is_none());

    Ok(())
}

#[test]
async fn a_stream_expectation_holds_the_request_to_its_id() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let event_service = open_event_service(&bmc).await?;

    bmc.expect(Expect::stream_events(
        SSE_URI,
        Some("41"),
        [(Some("42"), event(42))],
    ));
    let Err(error) = event_service.events_from(Some("40")).await else {
        panic!("the mock holds the request to the scripted id");
    };

    // The message names both the id asked for and the one scripted.
    let error = error.to_string();
    assert!(error.contains("unexpected stream"), "{}", error);
    assert!(error.contains("resumed after \"40\""), "{}", error);
    assert!(error.contains("Some(\"41\")"), "{}", error);

    Ok(())
}

async fn open_event_service(bmc: &Arc<Bmc>) -> Result<EventService<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    bmc.expect(Expect::get(&root_id, service_root(&root_id)));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(EVENT_SERVICE_ID, event_service()));
    Ok(service_root
        .event_service()
        .await?
        .expect("service root advertises an EventService"))
}

fn service_root(root_id: &ODataId) -> Value {
    json!({
        ODATA_ID: root_id,
        ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
        "Id": "RootService",
        "Name": "RootService",
        "EventService": { ODATA_ID: EVENT_SERVICE_ID },
        "Links": {
            "Sessions": { ODATA_ID: format!("{root_id}/SessionService/Sessions") }
        },
    })
}

fn event_service() -> Value {
    json!({
        ODATA_ID: EVENT_SERVICE_ID,
        ODATA_TYPE: EVENT_SERVICE_DATA_TYPE,
        "Id": "EventService",
        "Name": "Event Service",
        "ServerSentEventUri": SSE_URI,
    })
}

fn event(id: u32) -> Value {
    json!({
        ODATA_ID: format!("{SSE_URI}#/Event{id}"),
        ODATA_TYPE: EVENT_DATA_TYPE,
        "Id": id.to_string(),
        "Name": "Event Array",
        "Events": [{
            "MemberId": "0",
            "EventType": "Alert",
            "EventId": id.to_string(),
            "MessageId": "Example.1.0.TestEvent",
        }],
    })
}
