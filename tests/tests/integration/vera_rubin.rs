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
//! Vera Rubin (VR NVL72) integration tests.

use std::error::Error as StdError;
use std::sync::Arc;

use futures_util::TryStreamExt as _;
use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::event_service::EventStreamPayload;
use nv_redfish::schema::event::EventType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::nav_property::Reference;
use nv_redfish_core::ODataId;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;

const EVENT_SERVICE_DATA_TYPE: &str = "#EventService.v1_5_0.EventService";
const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_15_0.ServiceRoot";
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_22_0.ComputerSystem";

#[test]
async fn vera_rubin_sse_accepts_missing_and_present_event_type() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = test_ids();
    let event_service_id = format!("{}/EventService", ids.root_id);
    let sse_uri = format!("{event_service_id}/SSE");

    let service_root = expect_vera_rubin_service_root(
        bmc.clone(),
        &ids,
        json!({
            "EventService": { ODATA_ID: &event_service_id }
        }),
    )
    .await?;

    bmc.expect(Expect::get(
        &event_service_id,
        json!({
            ODATA_ID: &event_service_id,
            ODATA_TYPE: EVENT_SERVICE_DATA_TYPE,
            "Id": "EventService",
            "Name": "Event Service",
            "ServerSentEventUri": &sse_uri
        }),
    ));

    let event_service = service_root
        .event_service()
        .await?
        .expect("event service present");

    bmc.expect(Expect::stream(
        &sse_uri,
        json!([{
            ODATA_TYPE: "#Event.v1_7_0.Event",
            "Id": "1",
            "Name": "Event Array",
            "Events": [{
                "MemberId": "0",
                "MessageId": "Example.1.0.TestEvent"
            }, {
                "MemberId": "1",
                "EventType": "Alert",
                "MessageId": "Example.1.0.TestEvent"
            }]
        }]),
    ));

    let mut stream = event_service.events().await?;
    let payload = stream.try_next().await?.expect("event payload present");

    let EventStreamPayload::Event(event) = payload else {
        panic!("expected an event payload");
    };

    let mut records = event.events.iter();

    let patched = records
        .next()
        .expect("event record without EventType present")
        .get(bmc.as_ref())
        .await?;

    let preserved = records
        .next()
        .expect("event record with EventType present")
        .get(bmc.as_ref())
        .await?;

    assert_eq!(patched.event_type, EventType::Other);
    assert_eq!(preserved.event_type, EventType::Alert);

    Ok(())
}

#[test]
async fn vera_rubin_sse_uses_transport_id_for_missing_envelope_ids() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let ids = test_ids();
    let event_service_id = format!("{}/EventService", ids.root_id);
    let sse_uri = format!("{event_service_id}/SSE");

    let root = expect_vera_rubin_service_root(
        bmc.clone(),
        &ids,
        json!({"EventService": {ODATA_ID: &event_service_id}}),
    )
    .await?;

    bmc.expect(Expect::get(
        &event_service_id,
        json!({
            ODATA_ID: &event_service_id,
            ODATA_TYPE: EVENT_SERVICE_DATA_TYPE,
            "Id": "EventService", "Name": "Event Service",
            "ServerSentEventUri": &sse_uri
        }),
    ));

    let service = root.event_service().await?.expect("event service");

    // Synthetic capture shape: envelope IDs, MemberId, and record @odata.id are absent.
    // The record's EventId deliberately differs from the transport ID.
    let payload = json!({
        ODATA_TYPE: "#Event.v1_4_0.Event",
        "Name": "Event Log",
        "Events": [{
            "EventId": "record-7",
            "EventType": "Event",
            "EventTimestamp": "2026-01-01T00:00:00+00:00",
            "Message": "Test hardware event",
            "MessageId": "ResourceEvent.1.0.ResourceErrorsDetected",
            "MessageArgs": ["test-component", "test-error"],
            "MessageSeverity": "Critical",
            "LogEntry": {ODATA_ID: "/redfish/v1/Managers/1/LogServices/EventLog/Entries/7"},
            "OriginOfCondition": {ODATA_ID: "/redfish/v1/Systems/1"},
            "Oem": {"Vendor": {"DiagnosticData": "test-data"}}
        }]
    });

    // Repeated in-effect IDs are legal; normalization must not advance the cursor.
    bmc.expect(Expect::stream_events(
        &sse_uri,
        Some("40"),
        ["41", "41", "42"].map(|id| (Some(id), payload.clone())),
    ));

    let mut stream = service.events_from(Some("40")).await?;

    for id in ["41", "41", "42"] {
        let item = stream.try_next().await?.expect("event");

        assert_eq!(item.last_event_id.as_deref(), Some(id));

        let EventStreamPayload::Event(event) = item.data else {
            panic!("expected an Event payload");
        };

        let record = event.events[0].get(bmc.as_ref()).await?;

        assert_eq!(event.id, id);
        assert_eq!(event.odata_id.to_string(), format!("{sse_uri}#/Event{id}"));
        assert_eq!(record.event_id.as_deref(), Some("record-7"));
        assert_eq!(record.member_id, "record-7");
        assert_eq!(record.message.as_deref(), Some("Test hardware event"));

        assert_eq!(
            serde_json::to_value(Reference::from(
                record.log_entry.as_ref().expect("log reference")
            ))?,
            json!({"@odata.id": "/redfish/v1/Managers/1/LogServices/EventLog/Entries/7"})
        );

        assert_eq!(
            record.oem.as_ref().expect("OEM").additional_properties["Vendor"]["DiagnosticData"],
            "test-data"
        );
    }

    assert!(stream.try_next().await?.is_none());

    bmc.expect(Expect::stream_events(
        &sse_uri,
        None,
        [(Some("43"), payload)],
    ));

    let mut stream = service.events().await?;

    let EventStreamPayload::Event(event) = stream.try_next().await?.expect("event") else {
        panic!("expected an Event payload");
    };

    assert_eq!(event.id, "43");
    assert!(stream.try_next().await?.is_none());

    Ok(())
}

#[test]
async fn vera_rubin_composite_boot_order_is_normalized() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = test_ids();

    let system = get_system_0(
        bmc.clone(),
        &ids,
        json!({
            "Boot": {
                "BootOrder": [
                    "Boot0019: Ubuntu",
                    "Boot0010: UEFI HTTPv4 (MAC:F4204D494ECC)"
                ]
            }
        }),
    )
    .await?;

    let boot_order = system.boot_order().expect("boot order present");
    assert_eq!(boot_order.len(), 2);
    assert_eq!(*boot_order[0].inner(), "Boot0019");
    assert_eq!(*boot_order[1].inner(), "Boot0010");

    Ok(())
}

struct TestIds {
    root_id: ODataId,
    systems_id: String,
    system_0_id: String,
}

fn test_ids() -> TestIds {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_0_id = format!("{systems_id}/System_0");
    TestIds {
        root_id,
        systems_id,
        system_0_id,
    }
}

async fn get_system_0(
    bmc: Arc<Bmc>,
    ids: &TestIds,
    boot_fields: Value,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    let service_root = expect_vera_rubin_service_root(
        bmc.clone(),
        ids,
        json!({
            "Systems": { ODATA_ID: &ids.systems_id }
        }),
    )
    .await?;

    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: SYSTEM_COLLECTION_DATA_TYPE,
            "Name": "Computer System Collection",
            "Members": [
                { ODATA_ID: &ids.system_0_id }
            ]
        }),
    ));
    bmc.expect(Expect::get(
        &ids.system_0_id,
        system_0_payload(ids, boot_fields),
    ));

    let systems = service_root.systems().await?.unwrap();
    let mut members = systems.members().await?;
    members.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing System_0").into()
    })
}

async fn expect_vera_rubin_service_root(
    bmc: Arc<Bmc>,
    ids: &TestIds,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json_merge([
            &json!({
                ODATA_ID: &ids.root_id,
                ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
                "Id": "RootService",
                "Name": "Root Service",
                "Vendor": "NVIDIA",
                "Product": "VR NVL72",
                "RedfishVersion": "1.17.0",
                "ProtocolFeaturesSupported": {
                    "ExpandQuery": {
                        "NoLinks": true
                    }
                },
                "Links": {
                    "Sessions": {
                        ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                    }
                },
            }),
            &fields,
        ]),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

fn system_0_payload(ids: &TestIds, extra_fields: Value) -> Value {
    json_merge([
        &json!({
            ODATA_ID: &ids.system_0_id,
            ODATA_TYPE: SYSTEM_DATA_TYPE,
            "Id": "System_0",
            "Name": "System_0",
            "Status": {
                "Health": "OK",
                "State": "Enabled"
            },
        }),
        &extra_fields,
    ])
}
