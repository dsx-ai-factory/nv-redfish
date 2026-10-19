// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Repairs for event stream payloads, applied by
//! [`CompatBmc`](crate::CompatBmc) to each `Event` or `MetricReport` a
//! server-sent event stream delivers, before the document repairs. A
//! [`RulesBmc`](crate::RulesBmc) underneath has already applied the
//! caller's rules, so they see the payload as the device sent it.
//!
//! No device is known to give a payload or its event records the
//! `@odata.id`s the schema requires, so those are made up on every
//! platform; the other repairs follow the classified platform. An event's
//! `Id` can come from the SSE id it arrived under, which the document
//! itself does not carry, so the repairs take the stream's id along with
//! the document.
//!
//! OData ABNF reference:
//! <https://docs.oasis-open.org/odata/odata/v4.01/os/abnf/odata-abnf-construction-rules.txt>

use nv_redfish_core::odata::ODataType;
use serde_json::Map;
use serde_json::Value;

use crate::BmcQuirks;

const SSE_EVENT_BASE_ID: &str = "/redfish/v1/EventService/SSE";

/// A repair of one event record, given its position in `Events`.
type RecordRepair = fn(&mut Map<String, Value>, usize);

/// The repairs a platform's event stream payloads take.
#[derive(Clone)]
pub struct EventRepairs {
    /// Whether an event without an `Id` takes the SSE id.
    envelope_ids: bool,
    /// The record repairs, in the order they apply.
    records: Vec<RecordRepair>,
}

impl EventRepairs {
    /// The repairs for the classified platform `quirks`.
    pub fn new(quirks: &BmcQuirks) -> Self {
        let mut records: Vec<RecordRepair> = Vec::new();
        // An empty link goes before deciding whether a record is only a
        // reference.
        if quirks.event_service_sse_empty_log_entry() {
            records.push(empty_log_entry);
        }
        // The record's `@odata.id` below is made from its `MemberId`.
        if quirks.event_service_sse_no_member_id() {
            records.push(missing_record_member_id);
        }
        if quirks.event_service_sse_missing_event_type() {
            records.push(missing_event_type_to_other);
        }
        records.push(missing_record_odata_id);
        if quirks.event_service_sse_wrong_timestamp_offset() {
            records.push(compact_timestamp_offset);
        }
        Self {
            envelope_ids: quirks.event_service_sse_missing_envelope_ids(),
            records,
        }
    }

    /// Repairs `payload`, delivered under the SSE id `last_event_id`. A
    /// document that is neither an `Event` nor a `MetricReport` is left as
    /// it came.
    pub fn repair(&self, payload: &mut Value, last_event_id: Option<&str>) {
        if !matches!(
            ODataType::parse_from(payload).map(|kind| kind.type_name),
            Some("Event" | "MetricReport")
        ) {
            return;
        }
        if self.envelope_ids {
            missing_event_id(payload, last_event_id);
        }
        missing_event_odata_id(payload);
        for_each_event_record(payload, |record, index| {
            for repair in &self.records {
                repair(record, index);
            }
        });
    }
}

/// An event that carries neither `Id` nor `@odata.id` takes the SSE id it
/// arrived under. The stream's id is only read: it may be inherited from an
/// earlier event, and it stays as it is.
fn missing_event_id(payload: &mut Value, last_event_id: Option<&str>) {
    let Some(id) = last_event_id.filter(|id| !id.is_empty()) else {
        return;
    };
    if ODataType::parse_from(payload).is_none_or(|kind| kind.type_name != "Event") {
        return;
    }
    if let Some(payload) = payload.as_object_mut() {
        // Explicit identifiers, including null or malformed values, are not
        // repaired.
        if !payload.contains_key("Id") && !payload.contains_key("@odata.id") {
            payload.insert("Id".into(), Value::String(id.into()));
        }
    }
}

fn missing_event_odata_id(payload: &mut Value) {
    let Some(payload) = payload.as_object_mut() else {
        return;
    };
    if payload.contains_key("@odata.id") {
        return;
    }
    if let Some(event_id) = payload.get("Id").and_then(Value::as_str) {
        let generated_id = format!("{SSE_EVENT_BASE_ID}#/Event{event_id}");
        payload.insert("@odata.id".into(), Value::String(generated_id));
    }
}

fn for_each_event_record<F>(payload: &mut Value, mut repair: F)
where
    F: FnMut(&mut Map<String, Value>, usize),
{
    let Some(events) = payload
        .as_object_mut()
        .and_then(|payload| payload.get_mut("Events"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for (index, record) in events.iter_mut().enumerate() {
        if let Some(record) = record.as_object_mut() {
            repair(record, index);
        }
    }
}

/// Whether a record is only a reference: adding fields to one would make it
/// read as an expanded record.
fn is_reference(record: &Map<String, Value>) -> bool {
    record.len() == 1 && record.contains_key("@odata.id")
}

fn missing_record_member_id(record: &mut Map<String, Value>, index: usize) {
    if record.contains_key("MemberId") || is_reference(record) {
        return;
    }
    let member_id = record
        .get("EventId")
        .and_then(Value::as_str)
        .map_or_else(|| index.to_string(), ToOwned::to_owned);
    record.insert("MemberId".into(), Value::String(member_id));
}

fn missing_event_type_to_other(record: &mut Map<String, Value>, _index: usize) {
    if is_reference(record) {
        return;
    }
    if record.get("EventType").is_none() {
        record.insert("EventType".into(), Value::String("Other".into()));
    }
}

fn empty_log_entry(record: &mut Map<String, Value>, _index: usize) {
    if record
        .get("LogEntry")
        .and_then(Value::as_object)
        .is_some_and(Map::is_empty)
    {
        // An empty object contains no link; the optional navigation
        // property is omitted.
        record.remove("LogEntry");
    }
}

fn missing_record_odata_id(record: &mut Map<String, Value>, _index: usize) {
    if record.contains_key("@odata.id") {
        return;
    }
    if let Some(member_id) = record.get("MemberId").and_then(Value::as_str) {
        let generated_id = format!("{SSE_EVENT_BASE_ID}#/Events/{member_id}");
        record.insert("@odata.id".into(), Value::String(generated_id));
    }
}

fn compact_timestamp_offset(record: &mut Map<String, Value>, _index: usize) {
    if let Some(Value::String(timestamp)) = record.get("EventTimestamp") {
        if let Some(timestamp) = fix_timestamp_offset(timestamp) {
            record.insert("EventTimestamp".into(), Value::String(timestamp));
        }
    }
}

fn fix_timestamp_offset(input: &str) -> Option<String> {
    let sign_index = input.len().checked_sub(5)?;
    let suffix = input.get(sign_index..)?;
    let sign = suffix.chars().next()?;
    if sign != '+' && sign != '-' {
        return None;
    }
    let prefix = input.get(..(sign_index + 3))?;
    let minutes = input.get((sign_index + 3)..)?;
    Some(format!("{prefix}:{minutes}"))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::RootEvidence;

    /// The repairs of a Vera Rubin host BMC, which takes every record
    /// repair but the timestamp's, and the envelope ids.
    fn vera_rubin() -> EventRepairs {
        EventRepairs::new(&BmcQuirks::classify(&RootEvidence::from_root(
            &json!({ "Vendor": "NVIDIA", "Product": "VR NVL72" }),
        )))
    }

    fn repaired(repairs: &EventRepairs, mut payload: Value, last_event_id: Option<&str>) -> Value {
        repairs.repair(&mut payload, last_event_id);
        payload
    }

    // Synthetic data retains only the structure needed to reproduce the
    // failures.
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

    #[test]
    fn missing_envelope_id_repair_changes_only_the_absent_id() {
        let mut payload = synthetic_event();
        if let Some(payload) = payload.as_object_mut() {
            payload.remove("Id");
        }
        let mut expected = payload.clone();
        expected["Id"] = json!("transport-42");

        missing_event_id(&mut payload, Some("transport-42"));
        assert_eq!(payload, expected);

        let payload = repaired(&vera_rubin(), payload, Some("transport-42"));
        assert_eq!(
            payload["@odata.id"],
            "/redfish/v1/EventService/SSE#/Eventtransport-42"
        );
    }

    #[test]
    fn missing_envelope_id_repair_preserves_explicit_ids_and_other_payloads() {
        for payload in [
            json!({"@odata.type": "#MetricReport.v1_0_0.MetricReport"}),
            json!({}),
            json!({"@odata.type": null}),
            json!([]),
            Value::Null,
        ] {
            let mut repaired = payload.clone();
            missing_event_id(&mut repaired, Some("42"));
            assert_eq!(repaired, payload);
        }

        for key in ["Id", "@odata.id"] {
            for value in [Value::Null, json!(7), json!(""), json!("existing-id")] {
                let mut payload = json!({"@odata.type": "#Event.v1_4_0.Event"});
                payload[key] = value;
                let mut repaired = payload.clone();
                missing_event_id(&mut repaired, Some("42"));
                assert_eq!(repaired, payload);
            }
        }
    }

    #[test]
    fn an_unusable_cursor_leaves_the_envelope_without_ids() {
        let mut payload = synthetic_event();
        if let Some(payload) = payload.as_object_mut() {
            payload.remove("Id");
        }

        for cursor in [None, Some("")] {
            let repaired = repaired(&vera_rubin(), payload.clone(), cursor);
            assert!(repaired.get("Id").is_none());
            assert!(repaired.get("@odata.id").is_none());
        }
    }

    #[test]
    fn normalizes_compact_offset() {
        let fixed = fix_timestamp_offset("2017-11-23T17:17:42-0600");
        assert_eq!(fixed, Some("2017-11-23T17:17:42-06:00".to_owned()));
    }

    #[test]
    fn keeps_rfc3339_offset_unchanged() {
        assert_eq!(fix_timestamp_offset("2017-11-23T17:17:42-06:00"), None);
    }

    #[test]
    fn inserts_other_event_type_when_absent() {
        let mut payload = json!({
            "@odata.type": "#Event.v1_9_0.Event",
            "Events": [
                { "EventId": "1", "MessageId": "ResourceEvent.1.0.ResourceErrorsDetected" },
                { "EventId": "2", "EventType": "Alert" }
            ]
        });
        for_each_event_record(&mut payload, missing_event_type_to_other);
        assert_eq!(payload["Events"][0]["EventType"], "Other");
        assert_eq!(payload["Events"][1]["EventType"], "Alert");
    }

    #[test]
    fn patches_missing_member_id() {
        let mut payload = json!({ "Events": [{ "EventId": "88" }] });
        for_each_event_record(&mut payload, missing_record_member_id);
        assert_eq!(payload["Events"][0]["MemberId"], "88");
    }

    #[test]
    fn repairs_missing_ids_and_empty_log_references() {
        for (missing_member_id, empty_log_entry) in [(true, false), (false, true), (true, true)] {
            let mut payload = synthetic_event();
            if !missing_member_id {
                payload["Events"][0]["MemberId"] = json!("existing-member");
            }
            if !empty_log_entry {
                if let Some(record) = payload["Events"][0].as_object_mut() {
                    record.remove("LogEntry");
                }
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
            if let Some(record) = expected["Events"][0].as_object_mut() {
                record.remove("LogEntry");
            }

            assert_eq!(repaired(&vera_rubin(), payload, None), expected);
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

        assert_eq!(repaired(&vera_rubin(), payload.clone(), None), payload);
    }

    #[test]
    fn reference_only_records_are_left_as_references() {
        let mut payload = synthetic_event();
        payload["Events"] = json!([
            {"@odata.id": "/redfish/v1/EventService/SSE#/record"},
            {"@odata.id": "/redfish/v1/EventService/SSE#/record", "LogEntry": {}}
        ]);
        assert_eq!(
            repaired(&vera_rubin(), payload, None)["Events"],
            json!([
                {"@odata.id": "/redfish/v1/EventService/SSE#/record"},
                {"@odata.id": "/redfish/v1/EventService/SSE#/record"}
            ])
        );
    }

    #[test]
    fn empty_log_entry_repair_leaves_other_values_unchanged() {
        for log_entry in [
            json!({"unexpected": "value"}),
            json!([]),
            json!(""),
            Value::Null,
        ] {
            let mut payload = synthetic_event();
            payload["Events"][0]["LogEntry"] = log_entry.clone();
            assert_eq!(
                repaired(&vera_rubin(), payload, None)["Events"][0]["LogEntry"],
                log_entry
            );
        }
    }

    #[test]
    fn every_platform_makes_up_the_missing_odata_ids() {
        let unclassified = EventRepairs::new(&BmcQuirks::classify(&RootEvidence::default()));
        let mut payload = synthetic_event();
        payload["Events"][0]["MemberId"] = json!("7");
        let event = repaired(&unclassified, payload, None);
        assert_eq!(event["@odata.id"], "/redfish/v1/EventService/SSE#/Event1");
        assert_eq!(
            event["Events"][0]["@odata.id"],
            "/redfish/v1/EventService/SSE#/Events/7"
        );
        // Another stream's documents are left as they came.
        let other = json!({ "@odata.type": "#Chassis.v1_22_0.Chassis", "Id": "1" });
        assert_eq!(repaired(&unclassified, other.clone(), Some("9")), other);
    }
}
