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

//! Event Service entities and helpers.
//!
//! This module provides typed access to Redfish `EventService`.

mod patch;

use crate::patch_support::ReadPatchFn;
use crate::schema::event_service::EventService as EventServiceSchema;
use crate::Error;
use crate::NvBmc;
use crate::Resource;
use crate::ResourceSchema;
use crate::ServiceRoot;
use futures_util::future;
use futures_util::TryStreamExt as _;
use nv_redfish_core::odata::ODataType;
use nv_redfish_core::Bmc;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::StreamEvent;
use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value as JsonValue;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::metric_report::MetricReport;

#[doc(inline)]
pub use crate::schema::event::Event;

/// SSE payload that can contain either an `EventRecord` or a `MetricReport`.
#[derive(Debug)]
pub enum EventStreamPayload {
    /// Event record payload.
    Event(Event),
    /// Metric report payload.
    MetricReport(MetricReport),
}

impl<'de> Deserialize<'de> for EventStreamPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = JsonValue::deserialize(deserializer)?;
        let odata_type = ODataType::parse_from(&value)
            .ok_or_else(|| de::Error::missing_field("missing @odata.type in SSE payload"))?;

        if odata_type.type_name == "MetricReport" {
            let payload =
                serde_json::from_value::<MetricReport>(value).map_err(de::Error::custom)?;
            Ok(Self::MetricReport(payload))
        } else if odata_type.type_name == "Event" {
            let payload = serde_json::from_value::<Event>(value).map_err(de::Error::custom)?;
            Ok(Self::Event(payload))
        } else {
            Err(de::Error::custom(format!(
                "unsupported @odata.type in SSE payload: {}, should be either Event or MetricReport", odata_type.type_name
            )))
        }
    }
}

/// Event service.
///
/// Provides functions to inspect event delivery capabilities and parse
/// event payloads from `ServerSentEventUri`.
pub struct EventService<B: Bmc> {
    data: Arc<EventServiceSchema>,
    bmc: NvBmc<B>,
    sse_read_patches: Vec<ReadPatchFn>,
}

impl<B: Bmc> EventService<B> {
    /// Create a new event service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        if let Some(service_ref) = &root.root.event_service {
            let data = service_ref.get(bmc.as_ref()).await.map_err(Error::Bmc)?;

            let mut sse_read_patches = Vec::new();
            let mut sse_event_record_patches: Vec<patch::EventRecordPatchFn> = Vec::new();

            if bmc.quirks.event_service_sse_no_member_id() {
                sse_event_record_patches.push(patch::patch_missing_event_record_member_id);
            }
            if bmc.quirks.event_service_sse_missing_event_type() {
                sse_event_record_patches.push(patch::patch_missing_event_type_to_other);
            }
            if bmc.quirks.event_service_sse_no_odata_id() {
                let patch_event_id: ReadPatchFn =
                    Arc::new(patch::patch_missing_event_odata_id as fn(JsonValue) -> JsonValue);
                sse_read_patches.push(patch_event_id);
                sse_event_record_patches.push(patch::patch_missing_event_record_odata_id);
            }
            if bmc.quirks.event_service_sse_wrong_timestamp_offset() {
                sse_event_record_patches.push(patch::patch_compact_event_timestamp_offset);
            }

            if !sse_event_record_patches.is_empty() {
                let patch_event_records: ReadPatchFn = Arc::new(move |payload| {
                    patch::patch_event_records(payload, &sse_event_record_patches)
                });
                sse_read_patches.push(patch_event_records);
            }

            Ok(Some(Self {
                data,
                bmc: bmc.clone(),
                sse_read_patches,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the raw schema data for this event service.
    #[must_use]
    pub fn raw(&self) -> Arc<EventServiceSchema> {
        self.data.clone()
    }

    /// Open an SSE stream of Redfish event payloads.
    ///
    /// Payload kind is selected by `@odata.type`:
    /// - `Event` -> [`EventStreamPayload::Event`]
    /// - `MetricReport` -> [`EventStreamPayload::MetricReport`]
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `ServerSentEventUri` is not present in `EventService`
    /// - opening or consuming the SSE stream through the underlying BMC transport fails
    /// - deserializing patched SSE payload into [`EventStreamPayload`] fails
    pub async fn events(&self) -> Result<BoxTryStream<EventStreamPayload, Error<B>>, Error<B>>
    where
        B: 'static,
        B::Error: 'static,
    {
        let events = self.events_from(None).await?;
        Ok(Box::pin(events.map_ok(|event| event.data)))
    }

    /// Open an SSE stream of Redfish event payloads with the id each carries,
    /// resuming after `last_event_id`.
    ///
    /// Payloads are decoded as by [`EventService::events`]. Each item is a
    /// [`StreamEvent`] with the event id in effect, which a consumer that lost
    /// the stream passes back here to resume after the last payload it
    /// handled; `None` starts at the device's live position. A transport that
    /// does not surface event ids yields `None` for every id and cannot
    /// resume.
    ///
    /// # Errors
    ///
    /// As [`EventService::events`].
    pub async fn events_from(
        &self,
        last_event_id: Option<&str>,
    ) -> Result<BoxTryStream<StreamEvent<EventStreamPayload>, Error<B>>, Error<B>>
    where
        B: 'static,
        B::Error: 'static,
    {
        let stream_uri = self
            .data
            .server_sent_event_uri
            .as_ref()
            .ok_or(Error::EventServiceServerSentEventUriNotAvailable)?;

        let stream = self
            .bmc
            .as_ref()
            .stream_events::<JsonValue>(stream_uri, last_event_id)
            .await
            .map_err(Error::Bmc)?;

        let sse_read_patches = self.sse_read_patches.clone();
        let stream = stream.map_err(Error::Bmc).and_then(move |event| {
            let StreamEvent {
                last_event_id,
                data,
            } = event;
            let patched = sse_read_patches.iter().fold(data, |acc, patch| patch(acc));

            future::ready(
                serde_json::from_value::<EventStreamPayload>(patched)
                    .map(|data| StreamEvent {
                        last_event_id,
                        data,
                    })
                    .map_err(Error::Json),
            )
        });

        Ok(Box::pin(stream))
    }
}

impl<B: Bmc> Resource for EventService<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}

#[cfg(test)]
mod tests {
    use super::EventStreamPayload;

    #[test]
    fn event_stream_payload_deserializes_event_record() {
        let value = serde_json::json!({
            "@odata.id": "/redfish/v1/EventService/SSE#/Event1",
            "@odata.type": "#Event.v1_6_0.Event",
            "Id": "1",
            "Name": "Event Array",
            "Context": "ABCDEFGH",
            "Events": [
                    {
                    "@odata.id": "/redfish/v1/EventService/SSE#/Events/88",
                    "MemberId": "88",
                    "EventId": "88",
                    "EventTimestamp": "2026-02-19T03:55:29+00:00",
                    "EventType": "Alert",
                    "LogEntry": {
                        "@odata.id": "/redfish/v1/Systems/System_0/LogServices/EventLog/Entries/1674"
                    },
                    "Message": "The resource has been removed successfully.",
                    "MessageId": "ResourceEvent.1.2.ResourceRemoved",
                    "MessageSeverity": "OK",
                    "OriginOfCondition": {
                        "@odata.id": "/redfish/v1/AccountService/Accounts/1"
                    }
            }
            ]
        });

        let payload: EventStreamPayload =
            serde_json::from_value(value).expect("event payload must deserialize");
        assert!(matches!(payload, EventStreamPayload::Event(_)));
    }

    #[test]
    fn event_stream_payload_deserializes_metric_report() {
        let value = serde_json::json!({
                "@odata.id": "/redfish/v1/TelemetryService/MetricReports/AvgPlatformPowerUsage",
                "@odata.type": "#MetricReport.v1_3_0.MetricReport",
                "Id": "AvgPlatformPowerUsage",
                "Name": "Average Platform Power Usage metric report",
                "MetricReportDefinition": {
                    "@odata.id": "/redfish/v1/TelemetryService/MetricReportDefinitions/AvgPlatformPowerUsage"
                },
                "MetricValues": [
                    {
                        "MetricId": "AverageConsumedWatts",
                        "MetricValue": "100",
                        "Timestamp": "2016-11-08T12:25:00-05:00",
                        "MetricProperty": "/redfish/v1/Chassis/Tray_1/Power#/0/PowerConsumedWatts"
                    },
                    {
                        "MetricId": "AverageConsumedWatts",
                        "MetricValue": "94",
                        "Timestamp": "2016-11-08T13:25:00-05:00",
                        "MetricProperty": "/redfish/v1/Chassis/Tray_1/Power#/0/PowerConsumedWatts"
                    },
                    {
                        "MetricId": "AverageConsumedWatts",
                        "MetricValue": "100",
                        "Timestamp": "2016-11-08T14:25:00-05:00",
                        "MetricProperty": "/redfish/v1/Chassis/Tray_1/Power#/0/PowerConsumedWatts"
                    }
                ]
        });

        let payload: EventStreamPayload =
            serde_json::from_value(value).expect("metric report payload must deserialize");
        assert!(matches!(payload, EventStreamPayload::MetricReport(_)));
    }
}
