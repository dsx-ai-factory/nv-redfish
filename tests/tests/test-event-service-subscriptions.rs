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
//! The subscriptions an event service registered, listed from `Subscriptions`.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::event_service::EventService;
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
const DESTINATION: &str = "/redfish/v1/EventService/Subscriptions/1";
const OTHER_DESTINATION: &str = "/redfish/v1/EventService/Subscriptions/2";

type Bmc = HttpBmc<Client>;

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

/// An event service that lists `Subscriptions` at `subscriptions`, if given.
fn event_service(subscriptions: Option<&str>) -> Value {
    let mut service = json!({
        "@odata.id": EVENT_SERVICE,
        "@odata.type": "#EventService.v1_9_3.EventService",
        "Id": "EventService",
        "Name": "Event Service",
        "ServerSentEventUri": "/redfish/v1/EventService/SSE",
    });

    if let Some(subscriptions) = subscriptions {
        service["Subscriptions"] = json!({ "@odata.id": subscriptions });
    }

    service
}

/// A subscriptions collection holding `destinations`.
fn destinations(ids: &[&str]) -> Value {
    let members: Vec<Value> = ids.iter().map(|id| json!({ "@odata.id": id })).collect();

    json!({
        "@odata.id": SUBSCRIPTIONS,
        "@odata.type": "#EventDestinationCollection.EventDestinationCollection",
        "Name": "Event Subscriptions Collection",
        "Members": members,
    })
}

/// Serves `body` to a `GET` of `at`, as many times as it is requested.
async fn serve(server: &MockServer, at: &str, body: &Value) {
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
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

#[test]
async fn subscriptions_list_every_registered_destination() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve(&server, SERVICE_ROOT, &service_root()).await;
    serve(&server, EVENT_SERVICE, &event_service(Some(SUBSCRIPTIONS))).await;

    serve(
        &server,
        SUBSCRIPTIONS,
        &destinations(&[DESTINATION, OTHER_DESTINATION]),
    )
    .await;

    let event_service = open_event_service(&server).await?;

    assert_eq!(
        event_service.subscriptions().await?,
        [
            ODataId::from(DESTINATION.to_owned()),
            ODataId::from(OTHER_DESTINATION.to_owned())
        ]
    );

    Ok(())
}

#[test]
async fn an_event_service_without_subscriptions_lists_none() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve(&server, SERVICE_ROOT, &service_root()).await;
    serve(&server, EVENT_SERVICE, &event_service(None)).await;

    Mock::given(method("GET"))
        .and(path(SUBSCRIPTIONS))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;

    let event_service = open_event_service(&server).await?;

    assert!(event_service.subscriptions().await?.is_empty());

    Ok(())
}
