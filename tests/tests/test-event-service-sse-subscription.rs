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
//! An SSE subscription: the destination its open added, deleted by
//! `close_and_delete`.

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

use futures_util::TryStreamExt as _;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::event_service::CloseOutcome;
use nv_redfish::event_service::EventService;
use nv_redfish::event_service::EventStreamPayload;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use serde_json::json;
use serde_json::Value;
use tokio::test;
use url::Url;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

const SERVICE_ROOT: &str = "/redfish/v1";
const EVENT_SERVICE: &str = "/redfish/v1/EventService";
const SUBSCRIPTIONS: &str = "/redfish/v1/EventService/Subscriptions";
const SSE_URI: &str = "/redfish/v1/EventService/SSE";
const DESTINATION_1: &str = "/redfish/v1/EventService/Subscriptions/1";
const DESTINATION_2: &str = "/redfish/v1/EventService/Subscriptions/2";
const DESTINATION_3: &str = "/redfish/v1/EventService/Subscriptions/3";
const TASK: &str = "/redfish/v1/TaskService/Tasks/1";

/// One event record, as the service sends it over the stream.
const SSE_BODY: &str = concat!(
    "id: 7\n",
    "data: {\"@odata.id\":\"/redfish/v1/EventService/SSE#/Event7\",",
    "\"@odata.type\":\"#Event.v1_9_2.Event\",\"Id\":\"7\",\"Name\":\"Event Array\",",
    "\"Events\":[{\"MemberId\":\"0\",\"EventType\":\"Alert\",\"EventId\":\"7\",",
    "\"MessageId\":\"Example.1.0.TestEvent\"}]}\n\n"
);

type Bmc = HttpBmc<Client>;

fn id(path: &str) -> ODataId {
    ODataId::from(path.to_owned())
}

/// A service root that advertises an event service and nothing else.
fn service_root() -> Value {
    json!({
        "@odata.id": SERVICE_ROOT,
        "@odata.type": "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root Service",
        "EventService": { "@odata.id": EVENT_SERVICE },
        "Links": {
            "Sessions": { "@odata.id": "/redfish/v1/SessionService/Sessions" }
        },
    })
}

/// An event service that lists its subscriptions.
fn event_service() -> Value {
    json!({
        "@odata.id": EVENT_SERVICE,
        "@odata.type": "#EventService.v1_9_3.EventService",
        "Id": "EventService",
        "Name": "Event Service",
        "ServerSentEventUri": SSE_URI,
        "Subscriptions": { "@odata.id": SUBSCRIPTIONS },
    })
}

/// A subscriptions collection holding `ids`.
fn destinations(ids: &[&str]) -> Value {
    let members: Vec<Value> = ids.iter().map(|id| json!({ "@odata.id": id })).collect();

    json!({
        "@odata.id": SUBSCRIPTIONS,
        "@odata.type": "#EventDestinationCollection.EventDestinationCollection",
        "Name": "Event Subscriptions Collection",
        "Members": members,
    })
}

/// An `EventDestination` whose `SubscriptionType` is `subscription_type`.
fn destination(subscription_type: &str) -> Value {
    json!({
        "@odata.id": DESTINATION_2,
        "Id": "2",
        "Name": "Event Destination",
        "Context": "test-stream",
        "SubscriptionType": subscription_type,
    })
}

/// Serves `body` to a `GET` of `at`.
async fn serve(server: &MockServer, at: &str, body: &Value) {
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// Serves `before` to the first listing and `after` to every later one.
async fn serve_subscription_growth(server: &MockServer, before: &Value, after: &Value) {
    Mock::given(method("GET"))
        .and(path(SUBSCRIPTIONS))
        .respond_with(ResponseTemplate::new(200).set_body_json(before))
        .up_to_n_times(1)
        .mount(server)
        .await;

    Mock::given(method("GET"))
        .and(path(SUBSCRIPTIONS))
        .respond_with(ResponseTemplate::new(200).set_body_json(after))
        .mount(server)
        .await;
}

/// Serves one event over the stream.
async fn serve_stream(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path(SSE_URI))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SSE_BODY),
        )
        .mount(server)
        .await;
}

async fn open_event_service(server: &MockServer) -> Result<EventService<Bmc>, Box<dyn StdError>> {
    let bmc = Arc::new(HttpBmc::new(
        Client::new()?,
        Url::parse(&server.uri())?,
        BmcCredentials::new("root".into(), "password".into()),
        CacheSettings::default(),
    ));

    Ok(ServiceRoot::new(bmc)
        .await?
        .event_service()
        .await?
        .expect("the service root advertises an EventService"))
}

/// Serves the service, its event service, the subscription growth and the stream.
async fn serve_open(server: &MockServer, before: &Value, after: &Value) {
    serve(server, SERVICE_ROOT, &service_root()).await;
    serve(server, EVENT_SERVICE, &event_service()).await;

    serve_subscription_growth(server, before, after).await;
    serve_stream(server).await;
}

#[test]
async fn the_legacy_event_stream_reads_no_subscriptions() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve(&server, SERVICE_ROOT, &service_root()).await;
    serve(&server, EVENT_SERVICE, &event_service()).await;

    serve_stream(&server).await;

    Mock::given(method("GET"))
        .and(path(SUBSCRIPTIONS))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let mut events = open_event_service(&server).await?.events_from(None).await?;

    let event = events.try_next().await?.expect("one event");

    assert!(matches!(event.data, EventStreamPayload::Event(_)));

    Ok(())
}

#[test]
async fn the_one_destination_a_stream_added_is_its_own() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1, DESTINATION_2]),
    )
    .await;

    // The destination the open added reports itself as an SSE one.
    serve(&server, DESTINATION_2, &destination("SSE")).await;

    let mut subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    assert_eq!(subscription.destination(), Some(&id(DESTINATION_2)));

    let event = subscription.stream().try_next().await?.expect("one event");

    assert!(matches!(event.data, EventStreamPayload::Event(_)));

    Ok(())
}

#[test]
async fn a_stream_that_added_no_destination_tracks_none() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1]),
    )
    .await;

    Mock::given(method("DELETE"))
        .and(path(DESTINATION_1))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;

    let subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    assert_eq!(subscription.destination(), None);

    assert!(matches!(
        subscription.close_and_delete().await,
        CloseOutcome::SocketOnly
    ));

    Ok(())
}

#[test]
async fn a_concurrent_openers_destination_is_not_claimed() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1, DESTINATION_2, DESTINATION_3]),
    )
    .await;

    let subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    assert_eq!(subscription.destination(), None);

    Ok(())
}

#[test]
async fn a_non_sse_destination_is_not_claimed() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1, DESTINATION_2]),
    )
    .await;

    // Another client's event subscription, added while this stream opened.
    serve(&server, DESTINATION_2, &destination("RedfishEvent")).await;

    let subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    assert_eq!(subscription.destination(), None);

    Ok(())
}

#[test]
async fn closing_deletes_the_destination_the_stream_created() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1, DESTINATION_2]),
    )
    .await;

    serve(&server, DESTINATION_2, &destination("SSE")).await;

    Mock::given(method("DELETE"))
        .and(path(DESTINATION_2))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    assert!(matches!(
        subscription.close_and_delete().await,
        CloseOutcome::Deleted
    ));

    server.verify().await;

    Ok(())
}

#[test]
async fn an_asynchronous_delete_returns_its_task() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1, DESTINATION_2]),
    )
    .await;

    serve(&server, DESTINATION_2, &destination("SSE")).await;

    // The service runs the delete as a task.
    Mock::given(method("DELETE"))
        .and(path(DESTINATION_2))
        .respond_with(
            ResponseTemplate::new(202)
                .insert_header("Location", TASK)
                .insert_header("Retry-After", "15"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    let CloseOutcome::DeletePending(task) = subscription.close_and_delete().await else {
        panic!("an asynchronous delete must hand back its task");
    };

    assert_eq!(task.location.0, id(TASK));
    assert_eq!(task.retry_after, Some(Duration::from_secs(15)));

    Ok(())
}

#[test]
async fn deleting_a_reaped_destination_is_not_a_failure() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve_open(
        &server,
        &destinations(&[DESTINATION_1]),
        &destinations(&[DESTINATION_1, DESTINATION_2]),
    )
    .await;

    serve(&server, DESTINATION_2, &destination("SSE")).await;

    // Already reaped: the delete finds nothing.
    Mock::given(method("DELETE"))
        .and(path(DESTINATION_2))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;

    let subscription = open_event_service(&server)
        .await?
        .open_events_from(None)
        .await?;

    assert!(matches!(
        subscription.close_and_delete().await,
        CloseOutcome::AlreadyGone
    ));

    server.verify().await;

    Ok(())
}
