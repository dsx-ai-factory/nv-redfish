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

//! Event stream payloads are repaired by the compatibility layer before
//! they are decoded: repaired events decode, valid ones keep their ids and
//! fields, records that are only references stay references, and an event
//! that still does not decode is one error in the stream, not its end.

use std::error::Error as StdError;
use std::sync::Arc;

use futures_util::TryStreamExt as _;
use nv_redfish::event_service::EventService;
use nv_redfish::event_service::EventStreamPayload;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;

const ROOT: &str = "/redfish/v1";
const EVENT_SERVICE: &str = "/redfish/v1/EventService";
const SSE: &str = "/redfish/v1/EventService/SSE";

/// The event service of a Vera Rubin host BMC, whose platform takes every
/// event repair.
async fn vera_rubin_events(bmc: Arc<Bmc>) -> Result<EventService<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        ROOT,
        json!({
            ODATA_ID: ROOT,
            ODATA_TYPE: "#ServiceRoot.v1_15_0.ServiceRoot",
            "Id": "RootService",
            "Name": "Root Service",
            "Vendor": "NVIDIA",
            "Product": "VR NVL72",
            "Links": { "Sessions": { ODATA_ID: format!("{ROOT}/SessionService/Sessions") } },
            "EventService": { ODATA_ID: EVENT_SERVICE },
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;
    bmc.expect(Expect::get(
        EVENT_SERVICE,
        json!({
            ODATA_ID: EVENT_SERVICE,
            ODATA_TYPE: "#EventService.v1_5_0.EventService",
            "Id": "EventService",
            "Name": "Event Service",
            "ServerSentEventUri": SSE,
        }),
    ));
    Ok(root.event_service().await?.ok_or("event service")?)
}

// Synthetic data retains only the structure needed to reproduce the
// failures.
fn synthetic_event() -> Value {
    json!({
        ODATA_TYPE: "#Event.v1_9_0.Event",
        "Id": "1",
        "Name": "Event Log",
        "Events": [{
            "EventId": "1",
            "EventTimestamp": "2026-01-01T00:00:00+00:00",
            "EventType": "Event",
            "LogEntry": {},
            "Message": "Test hardware event",
            "MessageId": "",
            "MessageArgs": ["test-component", ""],
            "MessageSeverity": "Critical",
            "Oem": {"Vendor": {"ErrorId": "TEST-EVENT"}}
        }]
    })
}

#[test]
async fn repaired_events_decode_and_valid_ones_keep_their_fields() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let events = vera_rubin_events(bmc.clone()).await?;

    let mut valid = synthetic_event();
    valid["@odata.id"] = json!(format!("{SSE}#/existing-event"));
    valid["Events"][0]["MemberId"] = json!("existing-member");
    valid["Events"][0]["@odata.id"] = json!(format!("{SSE}#/existing-record"));
    valid["Events"][0]["LogEntry"] =
        json!({ ODATA_ID: "/redfish/v1/Managers/1/LogServices/EventLog/Entries/1" });
    let mut references = synthetic_event();
    references["Events"] = json!([
        { ODATA_ID: format!("{SSE}#/record") },
        { ODATA_ID: format!("{SSE}#/record"), "LogEntry": {} }
    ]);
    bmc.expect(Expect::stream_events(
        SSE,
        None,
        [
            (Some("1"), synthetic_event()),
            (Some("2"), valid),
            (Some("3"), references),
        ],
    ));
    let mut stream = events.events().await?;

    let EventStreamPayload::Event(repaired) = stream.try_next().await?.ok_or("event")? else {
        panic!("expected an Event payload");
    };
    assert_eq!(repaired.odata_id.to_string(), format!("{SSE}#/Event1"));

    let EventStreamPayload::Event(valid) = stream.try_next().await?.ok_or("event")? else {
        panic!("expected an Event payload");
    };
    assert_eq!(valid.odata_id.to_string(), format!("{SSE}#/existing-event"));
    let record = valid.events[0].get(bmc.as_ref()).await?;
    assert_eq!(record.member_id, "existing-member");
    assert!(record.log_entry.is_some());
    assert_eq!(
        record.oem.as_ref().ok_or("OEM")?.additional_properties["Vendor"]["ErrorId"],
        "TEST-EVENT"
    );

    let EventStreamPayload::Event(references) = stream.try_next().await?.ok_or("event")? else {
        panic!("expected an Event payload");
    };
    assert_eq!(references.events.len(), 2);

    assert!(stream.try_next().await?.is_none());
    Ok(())
}

#[test]
async fn an_event_that_does_not_decode_does_not_end_the_stream() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let events = vera_rubin_events(bmc.clone()).await?;

    // A link that is not one is left as it came, so the event does not
    // decode; neither does an event whose envelope id the stream cannot
    // supply.
    let mut broken_link = synthetic_event();
    broken_link["Events"][0]["LogEntry"] = json!({"unexpected": "value"});
    let mut no_id = synthetic_event();
    if let Some(event) = no_id.as_object_mut() {
        event.remove("Id");
    }
    bmc.expect(Expect::stream_events(
        SSE,
        None,
        [
            (Some("1"), broken_link),
            (Some(""), no_id),
            (Some("2"), synthetic_event()),
        ],
    ));
    let mut stream = events.events_from(None).await?;

    for _ in 0..2 {
        let item = stream.try_next().await;
        assert!(matches!(item, Err(Error::Decode(_))), "{:?}", item.err());
    }
    let event = stream.try_next().await?.ok_or("event")?;
    assert_eq!(event.last_event_id.as_deref(), Some("2"));
    assert!(matches!(event.data, EventStreamPayload::Event(_)));
    assert!(stream.try_next().await?.is_none());
    Ok(())
}
