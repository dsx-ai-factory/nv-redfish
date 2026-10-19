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

use crate::schema::event_destination::EventDestination as EventDestinationSchema;
use crate::schema::event_service::EventService as EventServiceSchema;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use futures_util::TryStreamExt as _;
use nv_redfish_core::odata::ODataType;
use nv_redfish_core::Bmc;
use nv_redfish_core::BmcError as _;
use nv_redfish_core::BmcErrorClass;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use nv_redfish_core::StreamEvent;
use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value as JsonValue;
use std::fmt;
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
}

impl<B: Bmc> EventService<B> {
    /// Create a new event service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        if let Some(service_ref) = &root.root.event_service {
            let data = service_ref.get(bmc.as_ref()).await.map_err(Error::from)?;

            Ok(Some(Self {
                data,
                bmc: bmc.clone(),
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

    /// List the `EventDestination`s in `Subscriptions`; empty if the service
    /// doesn't advertise it.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection cannot be fetched or deserialized.
    pub async fn subscriptions(&self) -> Result<Vec<ODataId>, Error<B>> {
        let Some(subscriptions) = &self.data.subscriptions else {
            return Ok(Vec::new());
        };

        let collection = subscriptions
            .get(self.bmc.as_ref())
            .await
            .map_err(Error::from)?;

        Ok(collection
            .members
            .iter()
            .map(|member| member.id().clone())
            .collect())
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
    /// - a payload, repaired by the compatibility layer, does not deserialize
    ///   into [`EventStreamPayload`]; that event is one error item and the
    ///   stream goes on
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
        let subscription = self.open_events_from(last_event_id).await?;
        Ok(subscription.into_stream())
    }

    /// Open an SSE stream of Redfish event payloads, tracking the subscription it creates.
    /// A failure to list `Subscriptions` leaves [`SseSubscription::destination`] `None`.
    ///
    /// # Errors
    ///
    /// As [`EventService::events`].
    pub async fn open_events_from(
        &self,
        last_event_id: Option<&str>,
    ) -> Result<SseSubscription<B>, Error<B>>
    where
        B: 'static,
        B::Error: 'static,
    {
        let before = self.subscriptions().await.ok();

        let stream_uri = self
            .data
            .server_sent_event_uri
            .as_ref()
            .ok_or(Error::EventServiceServerSentEventUriNotAvailable)?;

        // The compatibility layer repairs each payload under the SSE id it
        // arrived with before it is decoded.
        let stream = self
            .bmc
            .as_ref()
            .stream_events::<EventStreamPayload>(stream_uri, last_event_id)
            .await
            .map_err(Error::from)?;

        let after = self.subscriptions().await.ok();

        let destination = match (before, after) {
            (Some(before), Some(after)) => {
                let mut added: Vec<ODataId> = after
                    .iter()
                    .filter(|destination| !before.contains(destination))
                    .cloned()
                    .collect();

                // Two or more additions race with another client; none is ours.
                match added.len() {
                    1 => added.pop(),
                    _ => None,
                }
            }
            _ => None,
        };

        Ok(SseSubscription {
            stream: Box::pin(stream.map_err(Error::from)),
            destination,
            bmc: self.bmc.clone(),
        })
    }
}

/// An SSE stream plus the `EventDestination` it created.
///
/// Dropping it closes the connection; only [`SseSubscription::close`] deletes a
/// destination the service did not reap.
pub struct SseSubscription<B: Bmc> {
    stream: BoxTryStream<StreamEvent<EventStreamPayload>, Error<B>>,
    destination: Option<ODataId>,
    bmc: NvBmc<B>,
}

impl<B: Bmc> SseSubscription<B> {
    /// The destination this stream created, if the open identified one.
    #[must_use]
    pub const fn destination(&self) -> Option<&ODataId> {
        self.destination.as_ref()
    }

    /// The events of this stream, without giving up the subscription.
    pub fn stream(&mut self) -> &mut BoxTryStream<StreamEvent<EventStreamPayload>, Error<B>> {
        &mut self.stream
    }

    /// Take the events of this stream, tracking the subscription no further.
    #[must_use]
    pub fn into_stream(self) -> BoxTryStream<StreamEvent<EventStreamPayload>, Error<B>> {
        self.stream
    }

    /// Drop the stream, then DELETE the destination.
    pub async fn close(self) -> CloseOutcome<B> {
        let Self {
            stream,
            destination,
            bmc,
        } = self;

        drop(stream);

        let Some(destination) = destination else {
            return CloseOutcome::SocketOnly;
        };

        match bmc
            .as_ref()
            .delete::<NavProperty<EventDestinationSchema>>(&destination)
            .await
        {
            Ok(_) => CloseOutcome::Deleted,
            Err(error) => {
                let error: Error<B> = Error::from(error);

                let gone = matches!(
                    &error,
                    Error::Bmc(cause)
                        if cause.error_class()
                            == BmcErrorClass::HttpResponse { status: 404 }
                );

                if gone {
                    CloseOutcome::AlreadyGone
                } else {
                    CloseOutcome::DeleteFailed(error)
                }
            }
        }
    }
}

/// What closing an [`SseSubscription`] did to the service.
pub enum CloseOutcome<B: Bmc> {
    /// The connection closed; no destination to delete.
    SocketOnly,

    /// The destination was deleted.
    Deleted,

    /// The destination was already gone.
    AlreadyGone,

    /// The destination could not be deleted.
    DeleteFailed(Error<B>),
}

impl<B: Bmc> fmt::Debug for CloseOutcome<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SocketOnly => f.write_str("SocketOnly"),
            Self::Deleted => f.write_str("Deleted"),
            Self::AlreadyGone => f.write_str("AlreadyGone"),
            Self::DeleteFailed(error) => f.debug_tuple("DeleteFailed").field(error).finish(),
        }
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
