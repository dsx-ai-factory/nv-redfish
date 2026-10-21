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
//! Integration tests for SSE event records that omit `MemberId` and `EventType`.
//!
//! Both carry `Redfish.Required`, so a record missing either fails to
//! deserialize. `BmcQuirks` selects patches that supply them, but only for a
//! vendor it classifies.

use futures_util::StreamExt as _;
use nv_redfish::event_service::EventStreamPayload;
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
const EVENT_SERVICE_DATA_TYPE: &str = "#EventService.v1_9_3.EventService";
const EVENT_DATA_TYPE: &str = "#Event.v1_9_2.Event";
const EVENT_SERVICE_ID: &str = "/redfish/v1/EventService";
const SSE_URI: &str = "/redfish/v1/EventService/SSE";
const EVENT_ID: &str = "337";

/// Wiwynn ODM GB200 NVL trays omit both properties from every SSE event record.
#[test]
async fn wiwynn_sse_record_missing_member_id_and_event_type_is_supported(
) -> Result<(), Box<dyn StdError>> {
    let payload = first_stream_payload("WIWYNN", None).await?;

    assert!(
        matches!(payload, Ok(EventStreamPayload::Event(_))),
        "Wiwynn event record must deserialize, got {:?}",
        payload
    );

    Ok(())
}

/// The control: asserting a record deserializes proves nothing unless the same
/// record is known to fail when no quirk is selected.
#[test]
async fn unclassified_vendor_drops_the_same_record() -> Result<(), Box<dyn StdError>> {
    let payload = first_stream_payload("ACME", None).await?;

    // Serde reports whichever required property it reaches first, so assert on
    // the rejection rather than on which one.
    let error = payload.expect_err("record must fail when the vendor selects no patches");
    let error = format!("{error:?}");
    assert!(
        error.contains("missing field"),
        "must fail on a missing required property, not something incidental: {}",
        error
    );

    Ok(())
}

/// Regression guard for the NVIDIA-branded trays the patches were added for.
#[test]
async fn nvidia_sse_record_missing_member_id_and_event_type_is_supported(
) -> Result<(), Box<dyn StdError>> {
    let payload = first_stream_payload("NVIDIA", None).await?;

    assert!(
        matches!(payload, Ok(EventStreamPayload::Event(_))),
        "NVIDIA event record must deserialize, got {:?}",
        payload
    );

    Ok(())
}

/// NVLink switch tray BMCs (`Product: P3809`) classify as their own platform
/// rather than generic NVIDIA, and omit both properties the same way.
#[test]
async fn nvswitch_sse_record_missing_member_id_and_event_type_is_supported(
) -> Result<(), Box<dyn StdError>> {
    let payload = first_stream_payload("NVIDIA", Some("P3809")).await?;

    assert!(
        matches!(payload, Ok(EventStreamPayload::Event(_))),
        "NVSwitch event record must deserialize, got {:?}",
        payload
    );

    Ok(())
}

/// Returns the stream's first item still wrapped: the error case is a result
/// these tests assert on, not a failure.
async fn first_stream_payload(
    vendor: &str,
    product: Option<&str>,
) -> Result<Result<EventStreamPayload, impl std::fmt::Debug>, Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();

    bmc.expect(Expect::get(
        &root_id,
        service_root(&root_id, vendor, product),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(EVENT_SERVICE_ID, event_service()));
    let event_service = service_root
        .event_service()
        .await?
        .expect("service root advertises an EventService");

    bmc.expect(Expect::stream(SSE_URI, json!([event_payload()])));
    let mut events = event_service.events().await?;

    Ok(events.next().await.expect("stream yields one payload"))
}

fn service_root(root_id: &ODataId, vendor: &str, product: Option<&str>) -> Value {
    let mut root = json!({
        ODATA_ID: root_id,
        ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
        "Id": "RootService",
        "Name": "RootService",
        "Vendor": vendor,
        "EventService": { ODATA_ID: EVENT_SERVICE_ID },
        "Links": {
            "Sessions": { ODATA_ID: format!("{root_id}/SessionService/Sessions") }
        },
    });
    if let Some(product) = product {
        root["Product"] = json!(product);
    }
    root
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

/// A platform-error record as the GB200 NVL trays emit it — note the absent
/// `MemberId` and `EventType`.
fn event_payload() -> Value {
    json!({
        ODATA_ID: format!("{SSE_URI}#/Event1"),
        ODATA_TYPE: EVENT_DATA_TYPE,
        "Id": EVENT_ID,
        "Name": "Event Log",
        "Events": [
            {
                "EventId": EVENT_ID,
                "EventTimestamp": "2026-07-31T14:21:01+00:00",
                "MessageId": "Platform.1.0.PlatformError",
                "MessageSeverity": "Critical",
            }
        ],
    })
}
