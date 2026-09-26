// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Patches for EventService SSE payloads.
//!
//! OData ABNF reference:
//! <https://docs.oasis-open.org/odata/odata/v4.01/os/abnf/odata-abnf-construction-rules.txt>

use serde_json::map::Map as JsonMap;
use serde_json::Value as JsonValue;

const SSE_EVENT_BASE_ID: &str = "/redfish/v1/EventService/SSE";

pub(super) type EventRecordPatchFn = fn(&mut JsonMap<String, JsonValue>, usize);

pub(super) fn patch_missing_event_odata_id(mut value: JsonValue) -> JsonValue {
    let Some(payload) = value.as_object_mut() else {
        return value;
    };

    if payload.contains_key("@odata.id") {
        return value;
    }

    if let Some(event_id) = payload.get("Id").and_then(JsonValue::as_str) {
        let generated_id = format!("{SSE_EVENT_BASE_ID}#/Event{event_id}");
        payload.insert("@odata.id".to_string(), JsonValue::String(generated_id));
    }
    value
}

pub(super) fn patch_event_records(
    mut value: JsonValue,
    patches: &[EventRecordPatchFn],
) -> JsonValue {
    for_each_event_record(&mut value, |record_obj, index| {
        for patch in patches {
            patch(record_obj, index);
        }
    });
    value
}

pub(super) fn for_each_event_record<F>(value: &mut JsonValue, mut patch: F)
where
    F: FnMut(&mut JsonMap<String, JsonValue>, usize),
{
    let Some(payload) = value.as_object_mut() else {
        return;
    };

    let Some(events) = payload.get_mut("Events").and_then(JsonValue::as_array_mut) else {
        return;
    };

    for (index, record) in events.iter_mut().enumerate() {
        let Some(record_obj) = record.as_object_mut() else {
            continue;
        };
        patch(record_obj, index);
    }
}

pub(super) fn patch_missing_event_record_member_id(
    value: &mut JsonMap<String, JsonValue>,
    index: usize,
) {
    // Adding fields to a reference would make it parse as an expanded record.
    if value.contains_key("MemberId") || (value.len() == 1 && value.contains_key("@odata.id")) {
        return;
    }

    let fallback_member_id = value
        .get("EventId")
        .and_then(JsonValue::as_str)
        .map_or_else(|| index.to_string(), ToOwned::to_owned);
    value.insert(
        "MemberId".to_string(),
        JsonValue::String(fallback_member_id),
    );
}

pub(super) fn patch_missing_event_type_to_other(
    value: &mut JsonMap<String, JsonValue>,
    _index: usize,
) {
    // Adding fields to a reference would make it parse as an expanded record.
    if value.len() == 1 && value.contains_key("@odata.id") {
        return;
    }

    if value.get("EventType").is_none() {
        value.insert(
            "EventType".to_string(),
            JsonValue::String("Other".to_string()),
        );
    }
}

pub(super) fn patch_empty_log_entry(value: &mut JsonMap<String, JsonValue>, _index: usize) {
    if value
        .get("LogEntry")
        .and_then(JsonValue::as_object)
        .is_some_and(JsonMap::is_empty)
    {
        // An empty object contains no link; omit the optional navigation property.
        value.remove("LogEntry");
    }
}

pub(super) fn patch_missing_event_record_odata_id(
    value: &mut JsonMap<String, JsonValue>,
    _index: usize,
) {
    if value.contains_key("@odata.id") {
        return;
    }

    if let Some(member_id) = value.get("MemberId").and_then(JsonValue::as_str) {
        let generated_id = format!("{SSE_EVENT_BASE_ID}#/Events/{member_id}");
        value.insert("@odata.id".to_string(), JsonValue::String(generated_id));
    }
}

pub(super) fn patch_compact_event_timestamp_offset(
    value: &mut JsonMap<String, JsonValue>,
    _index: usize,
) {
    if let Some(JsonValue::String(timestamp)) = value.get("EventTimestamp") {
        if let Some(timestamp) = fix_timestamp_offset(timestamp) {
            value.insert("EventTimestamp".to_string(), JsonValue::String(timestamp));
        }
    }
}

fn fix_timestamp_offset(input: &str) -> Option<String> {
    let sign_index = input.len().checked_sub(5)?;
    let suffix = input.get(sign_index..)?;
    let mut chars = suffix.chars();
    let sign = chars.next()?;
    if sign != '+' && sign != '-' {
        return None;
    }

    let prefix = input.get(..(sign_index + 3))?;
    let minutes = input.get((sign_index + 3)..)?;
    Some(format!("{prefix}:{minutes}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_service::EventStreamPayload;
    use crate::schema::event::EventRecord;

    use serde_json::json;
    use serde_json::Value;

    // Exercise the repairs directly; EventService's async wiring is not under test.
    const ID_AND_LOG_PATCHES: [EventRecordPatchFn; 4] = [
        patch_empty_log_entry,
        patch_missing_event_record_member_id,
        patch_missing_event_type_to_other,
        patch_missing_event_record_odata_id,
    ];

    #[test]
    fn normalizes_compact_offset() {
        let fixed = fix_timestamp_offset("2017-11-23T17:17:42-0600");
        assert_eq!(fixed, Some("2017-11-23T17:17:42-06:00".to_string()));
    }

    #[test]
    fn keeps_rfc3339_offset_unchanged() {
        assert_eq!(fix_timestamp_offset("2017-11-23T17:17:42-06:00"), None);
    }

    #[test]
    fn inserts_other_event_type_when_absent() {
        let payload = json!({
            "Events": [
                {
                    "EventId": "1",
                    "MessageId": "ResourceEvent.1.0.ResourceErrorsDetected"
                },
                {
                    "EventId": "2",
                    "EventType": "Alert"
                }
            ]
        });

        let payload = patch_event_records(
            payload,
            &[patch_missing_event_type_to_other as EventRecordPatchFn],
        );

        let events = payload
            .get("Events")
            .and_then(serde_json::Value::as_array)
            .expect("events array");
        assert_eq!(
            events[0]
                .get("EventType")
                .and_then(serde_json::Value::as_str),
            Some("Other")
        );
        assert_eq!(
            events[1]
                .get("EventType")
                .and_then(serde_json::Value::as_str),
            Some("Alert")
        );
    }

    #[test]
    fn patches_missing_member_id() {
        let payload = json!({
            "Events": [
                {
                    "EventId": "88"
                }
            ]
        });

        let payload = patch_event_records(
            payload,
            &[patch_missing_event_record_member_id as EventRecordPatchFn],
        );

        let member_id = payload
            .get("Events")
            .and_then(serde_json::Value::as_array)
            .and_then(|events| events.first())
            .and_then(|event| event.get("MemberId"))
            .and_then(serde_json::Value::as_str);
        assert_eq!(member_id, Some("88"));
    }

    #[test]
    fn repairs_missing_ids_and_empty_log_references() {
        for (missing_member_id, empty_log_entry) in [(true, false), (false, true), (true, true)] {
            let mut payload = synthetic_event();

            if !missing_member_id {
                payload["Events"][0]["MemberId"] = json!("existing-member");
            }

            if !empty_log_entry {
                payload["Events"][0]
                    .as_object_mut()
                    .expect("event record")
                    .remove("LogEntry");
            }

            let member_id = if missing_member_id {
                "1"
            } else {
                "existing-member"
            };

            let mut expected = payload.clone();
            expected["@odata.id"] = json!("/redfish/v1/EventService/SSE#/Event1");
            expected["Events"][0]["MemberId"] = json!(member_id);
            expected["Events"][0]["@odata.id"] =
                json!(format!("/redfish/v1/EventService/SSE#/Events/{member_id}"));

            expected["Events"][0]
                .as_object_mut()
                .expect("event record")
                .remove("LogEntry");

            let patched =
                patch_event_records(patch_missing_event_odata_id(payload), &ID_AND_LOG_PATCHES);

            let decoded: EventStreamPayload =
                serde_json::from_value(patched.clone()).expect("repaired event must decode");

            assert_eq!(patched, expected);
            assert!(matches!(decoded, EventStreamPayload::Event(_)));
        }
    }

    #[test]
    fn repairs_preserve_valid_ids_references_and_event_fields() {
        let mut payload = synthetic_event();
        payload["@odata.id"] = json!("/redfish/v1/EventService/SSE#/existing-event");
        payload["Events"][0]["MemberId"] = json!("existing-member");
        payload["Events"][0]["@odata.id"] = json!("/redfish/v1/EventService/SSE#/existing-record");
        payload["Events"][0]["LogEntry"] =
            json!({"@odata.id": "/redfish/v1/Managers/1/LogServices/EventLog/Entries/1"});

        let patched = patch_event_records(
            patch_missing_event_odata_id(payload.clone()),
            &ID_AND_LOG_PATCHES,
        );

        let decoded: EventStreamPayload =
            serde_json::from_value(patched.clone()).expect("valid event must decode");

        let record: EventRecord =
            serde_json::from_value(patched["Events"][0].clone()).expect("record must decode");

        assert_eq!(patched, payload);
        assert!(matches!(decoded, EventStreamPayload::Event(_)));
        assert!(record.log_entry.is_some());

        assert_eq!(
            record.oem.expect("OEM extensions").additional_properties["Vendor"]["ErrorId"],
            "TEST-EVENT"
        );
    }

    #[test]
    fn missing_member_id_repair_preserves_reference_only_records() {
        let mut payload = synthetic_event();
        payload["Events"] = json!([{"@odata.id": "/redfish/v1/EventService/SSE#/record"}]);

        let patched = patch_event_records(
            patch_missing_event_odata_id(payload.clone()),
            &ID_AND_LOG_PATCHES,
        );

        let decoded: EventStreamPayload =
            serde_json::from_value(patched.clone()).expect("reference must decode");

        assert_eq!(patched["Events"], payload["Events"]);
        assert!(matches!(decoded, EventStreamPayload::Event(_)));
    }

    #[test]
    fn empty_log_entry_repair_preserves_reference_only_records() {
        let mut payload = synthetic_event();

        payload["Events"] = json!([{
            "@odata.id": "/redfish/v1/EventService/SSE#/record",
            "LogEntry": {}
        }]);

        let patched =
            patch_event_records(patch_missing_event_odata_id(payload), &ID_AND_LOG_PATCHES);

        assert_eq!(
            patched["Events"],
            json!([{"@odata.id": "/redfish/v1/EventService/SSE#/record"}])
        );

        let decoded: EventStreamPayload =
            serde_json::from_value(patched).expect("reference must decode");

        assert!(matches!(decoded, EventStreamPayload::Event(_)));
    }

    #[test]
    fn empty_log_entry_repair_leaves_other_values_unchanged() {
        for (log_entry, should_decode) in [
            (json!({"unexpected": "value"}), false),
            (json!([]), false),
            (json!(""), false),
            (Value::Null, true),
        ] {
            let mut payload = synthetic_event();
            payload["Events"][0]["LogEntry"] = log_entry.clone();

            let patched =
                patch_event_records(patch_missing_event_odata_id(payload), &ID_AND_LOG_PATCHES);

            let decoded = serde_json::from_value::<EventStreamPayload>(patched.clone());

            assert_eq!(patched["Events"][0]["LogEntry"], log_entry);
            assert_eq!(decoded.is_ok(), should_decode, "{decoded:?}");
        }
    }

    // Synthetic data retains only the structure needed to reproduce the failures.
    fn synthetic_event() -> Value {
        json!({
            "@odata.type": "#Event.v1_9_0.Event",
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
}
