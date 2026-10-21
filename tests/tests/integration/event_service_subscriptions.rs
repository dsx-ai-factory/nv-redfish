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
use nv_redfish::event_service::SubscriptionType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use serde_json::json;
use serde_json::Value;
use tokio::test;
use url::Url;
use wiremock::matchers::header;
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
const THIRD_DESTINATION: &str = "/redfish/v1/EventService/Subscriptions/3";

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

/// A destination at `id` reporting `subscription_type`, `null` if `None`.
fn destination(id: &str, subscription_type: Option<&str>) -> Value {
    json!({
        "@odata.id": id,
        "@odata.type": "#EventDestination.v1_14_0.EventDestination",
        "Id": id.rsplit('/').next(),
        "Name": "Event Subscription",
        "Context": "",
        "Protocol": "Redfish",
        "SubscriptionType": subscription_type,
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

/// Serves `status` with no body to a `GET` of `at`.
async fn fail(server: &MockServer, at: &str, status: u16) {
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(ResponseTemplate::new(status))
        .mount(server)
        .await;
}

/// Serves the root, an event service, and a `Subscriptions` collection
/// holding `members`, then opens the event service. Callers serve each member.
async fn open_with_subscriptions(
    server: &MockServer,
    members: &[&str],
) -> Result<EventService<Bmc>, Box<dyn StdError>> {
    serve(server, SERVICE_ROOT, &service_root()).await;
    serve(server, EVENT_SERVICE, &event_service(Some(SUBSCRIPTIONS))).await;
    serve(server, SUBSCRIPTIONS, &destinations(members)).await;

    open_event_service(server).await
}

/// Each listed destination's `@odata.id` and reported `SubscriptionType`.
async fn listing(
    event_service: &EventService<Bmc>,
) -> Result<Vec<(String, Option<SubscriptionType>)>, Box<dyn StdError>> {
    Ok(event_service
        .subscriptions()
        .await?
        .iter()
        .map(|destination| {
            (
                destination.raw().odata_id().to_string(),
                destination.subscription_type(),
            )
        })
        .collect())
}

#[test]
async fn missing_subscription_type_defaults_for_referenced_and_expanded_members(
) -> Result<(), Box<dyn StdError>> {
    for (expanded, explicit, expected) in [
        (false, None, None),
        (true, None, None),
        (
            false,
            Some("FutureType"),
            Some(SubscriptionType::UnsupportedValue),
        ),
        (
            true,
            Some("FutureType"),
            Some(SubscriptionType::UnsupportedValue),
        ),
    ] {
        let server = MockServer::start().await;
        let version = if expanded { "v1_17_0" } else { "v1_2_0" };
        let mut legacy = destination(DESTINATION, None);
        legacy["@odata.type"] = json!(format!("#EventDestination.{version}.EventDestination"));
        legacy
            .as_object_mut()
            .expect("destination")
            .remove("SubscriptionType");

        let explicit = destination(THIRD_DESTINATION, explicit);
        let mut collection = destinations(&[DESTINATION, OTHER_DESTINATION, THIRD_DESTINATION]);
        collection["@odata.etag"] = json!("\"subscriptions\"");

        if expanded {
            collection["Members"][0] = legacy.clone();
            collection["Members"][2] = explicit.clone();

            Mock::given(method("GET"))
                .and(path(DESTINATION))
                .respond_with(ResponseTemplate::new(500))
                .expect(0)
                .mount(&server)
                .await;
        } else {
            serve(&server, DESTINATION, &legacy).await;
        }

        serve(&server, SERVICE_ROOT, &service_root()).await;
        serve(&server, EVENT_SERVICE, &event_service(Some(SUBSCRIPTIONS))).await;
        serve(&server, SUBSCRIPTIONS, &collection).await;

        Mock::given(method("GET"))
            .and(path(SUBSCRIPTIONS))
            .and(header("if-none-match", "\"subscriptions\""))
            .respond_with(ResponseTemplate::new(304))
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;

        serve(
            &server,
            OTHER_DESTINATION,
            &destination(OTHER_DESTINATION, Some("SSE")),
        )
        .await;

        serve(&server, THIRD_DESTINATION, &explicit).await;

        let event_service = open_event_service(&server).await?;
        let listed = listing(&event_service).await?;

        assert_eq!(
            listed,
            [
                (DESTINATION.to_owned(), Some(SubscriptionType::RedfishEvent)),
                (OTHER_DESTINATION.to_owned(), Some(SubscriptionType::Sse)),
                (THIRD_DESTINATION.to_owned(), expected),
            ]
        );

        // Revalidating the cached collection must preserve repaired members.
        assert_eq!(listing(&event_service).await?, listed);
    }

    Ok(())
}

#[test]
async fn annotated_subscriptions_link_does_not_prevent_opening_event_service(
) -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    let mut service = event_service(Some(SUBSCRIPTIONS));
    service["Subscriptions"]["Members@odata.count"] = json!(0);

    serve(&server, SERVICE_ROOT, &service_root()).await;
    serve(&server, EVENT_SERVICE, &service).await;
    serve(&server, SUBSCRIPTIONS, &destinations(&[])).await;

    let event_service = open_event_service(&server).await?;

    // Keep the public link type and field access supported before listing was added.
    let raw = event_service.raw();

    let link: &nv_redfish_core::nav_property::ReferenceLeaf =
        raw.subscriptions.as_ref().expect("subscriptions link");

    assert_eq!(link.odata_id.to_string(), SUBSCRIPTIONS);
    assert!(event_service.subscriptions().await?.is_empty());

    Ok(())
}

#[test]
async fn subscription_defaults_do_not_hide_invalid_fields() -> Result<(), Box<dyn StdError>> {
    for expanded in [false, true] {
        for (field, value) in [
            ("SubscriptionType", Some(json!(42))),
            ("SubscriptionType", Some(json!(false))),
            ("SubscriptionType", Some(json!({}))),
            ("SubscriptionType", Some(json!([]))),
            ("Context", None),
        ] {
            let server = MockServer::start().await;
            let mut invalid = destination(DESTINATION, Some("SSE"));

            if let Some(value) = value {
                invalid[field] = value;
            } else {
                invalid.as_object_mut().expect("destination").remove(field);
                invalid
                    .as_object_mut()
                    .expect("destination")
                    .remove("SubscriptionType");
            }

            let mut collection = destinations(&[DESTINATION]);

            if expanded {
                collection["Members"][0] = invalid.clone();
            }

            serve(&server, SERVICE_ROOT, &service_root()).await;
            serve(&server, EVENT_SERVICE, &event_service(Some(SUBSCRIPTIONS))).await;
            serve(&server, SUBSCRIPTIONS, &collection).await;
            serve(&server, DESTINATION, &invalid).await;

            let event_service = open_event_service(&server).await?;

            let error = event_service
                .subscriptions()
                .await
                .err()
                .expect("invalid destination");

            assert!(matches!(error, nv_redfish::Error::Decode(_)), "{}", error);
        }
    }

    Ok(())
}

#[test]
async fn subscriptions_report_each_destination_type_in_order() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve(&server, DESTINATION, &destination(DESTINATION, Some("SSE"))).await;

    serve(
        &server,
        OTHER_DESTINATION,
        &destination(OTHER_DESTINATION, Some("RedfishEvent")),
    )
    .await;

    serve(
        &server,
        THIRD_DESTINATION,
        &destination(THIRD_DESTINATION, None),
    )
    .await;

    let event_service = open_with_subscriptions(
        &server,
        &[DESTINATION, OTHER_DESTINATION, THIRD_DESTINATION],
    )
    .await?;

    assert_eq!(
        listing(&event_service).await?,
        [
            (DESTINATION.to_owned(), Some(SubscriptionType::Sse)),
            (
                OTHER_DESTINATION.to_owned(),
                Some(SubscriptionType::RedfishEvent)
            ),
            (THIRD_DESTINATION.to_owned(), None),
        ]
    );

    Ok(())
}

#[test]
async fn a_destination_deleted_mid_listing_is_omitted() -> Result<(), Box<dyn StdError>> {
    for status in [404, 410] {
        let server = MockServer::start().await;

        fail(&server, DESTINATION, status).await;

        serve(
            &server,
            OTHER_DESTINATION,
            &destination(OTHER_DESTINATION, Some("SSE")),
        )
        .await;

        let event_service =
            open_with_subscriptions(&server, &[DESTINATION, OTHER_DESTINATION]).await?;

        assert_eq!(
            listing(&event_service).await?,
            [(OTHER_DESTINATION.to_owned(), Some(SubscriptionType::Sse))]
        );
    }

    Ok(())
}

#[test]
async fn a_destination_that_fails_to_load_fails_the_listing() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    fail(&server, DESTINATION, 500).await;

    serve(
        &server,
        OTHER_DESTINATION,
        &destination(OTHER_DESTINATION, Some("SSE")),
    )
    .await;

    let event_service = open_with_subscriptions(&server, &[DESTINATION, OTHER_DESTINATION]).await?;

    assert!(event_service.subscriptions().await.is_err());

    Ok(())
}

#[test]
async fn an_expanded_subscriptions_copy_is_not_the_listing() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve(&server, SERVICE_ROOT, &service_root()).await;

    let mut expanded = event_service(Some(SUBSCRIPTIONS));
    expanded["Subscriptions"] = destinations(&[DESTINATION]);
    serve(&server, EVENT_SERVICE, &expanded).await;

    // The service holds a destination the expanded copy predates.
    serve(
        &server,
        SUBSCRIPTIONS,
        &destinations(&[DESTINATION, OTHER_DESTINATION]),
    )
    .await;

    serve(&server, DESTINATION, &destination(DESTINATION, Some("SSE"))).await;

    serve(
        &server,
        OTHER_DESTINATION,
        &destination(OTHER_DESTINATION, Some("SSE")),
    )
    .await;

    let event_service = open_event_service(&server).await?;

    assert_eq!(
        listing(&event_service).await?,
        [
            (DESTINATION.to_owned(), Some(SubscriptionType::Sse)),
            (OTHER_DESTINATION.to_owned(), Some(SubscriptionType::Sse)),
        ]
    );

    Ok(())
}

#[test]
async fn a_listed_destination_deletes_itself() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;

    serve(&server, DESTINATION, &destination(DESTINATION, Some("SSE"))).await;

    Mock::given(method("DELETE"))
        .and(path(DESTINATION))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let event_service = open_with_subscriptions(&server, &[DESTINATION]).await?;
    let destinations = event_service.subscriptions().await?;

    let [destination] = destinations.as_slice() else {
        panic!("one destination expected, got {}", destinations.len());
    };

    let result = destination.delete().await;

    assert!(matches!(result, Ok(ModificationResponse::Empty)));

    Ok(())
}

#[test]
async fn delete_applies_defaults_to_the_returned_destination() -> Result<(), Box<dyn StdError>> {
    for (reported_type, expected) in [
        (None, Some(SubscriptionType::RedfishEvent)),
        (Some(json!(null)), None),
        (Some(json!("SSE")), Some(SubscriptionType::Sse)),
        (
            Some(json!("FutureType")),
            Some(SubscriptionType::UnsupportedValue),
        ),
    ] {
        let server = MockServer::start().await;
        let mut body = destination(DESTINATION, None);

        if let Some(value) = reported_type {
            body["SubscriptionType"] = value;
        } else {
            body.as_object_mut()
                .expect("destination")
                .remove("SubscriptionType");
        }

        body["Context"] = json!("test context");
        body["Oem"] = json!({"Example": {"Data": [1, 2, 3]}});

        // A returned resource must not trigger a second GET after deletion.
        Mock::given(method("GET"))
            .and(path(DESTINATION))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        Mock::given(method("DELETE"))
            .and(path(DESTINATION))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let service = open_with_subscriptions(&server, &[DESTINATION]).await?;
        let listed = service.subscriptions().await?;
        let destination = listed.first().expect("one destination");

        assert_eq!(destination.subscription_type(), expected);

        let ModificationResponse::Entity(deleted) = destination.delete().await? else {
            panic!("expected the returned destination");
        };

        assert_eq!(deleted.subscription_type(), expected);

        let raw = deleted.raw();

        assert_eq!(raw.odata_id().to_string(), DESTINATION);
        assert_eq!(raw.context.as_deref(), Some("test context"));

        assert_eq!(
            raw.oem.as_ref().expect("OEM data").additional_properties["Example"],
            json!({"Data": [1, 2, 3]})
        );
    }

    Ok(())
}

#[test]
async fn delete_preserves_success_message_and_task_outcomes() -> Result<(), Box<dyn StdError>> {
    for status in [200, 202] {
        let server = MockServer::start().await;

        serve(&server, DESTINATION, &destination(DESTINATION, Some("SSE"))).await;

        let response = if status == 200 {
            ResponseTemplate::new(status).set_body_json(json!({
                "error": {"code": "Base.1.0.Success", "message": "Completed"}
            }))
        } else {
            ResponseTemplate::new(status)
                .insert_header("Location", "/redfish/v1/TaskService/Tasks/1")
        };

        Mock::given(method("DELETE"))
            .and(path(DESTINATION))
            .respond_with(response)
            .expect(1)
            .mount(&server)
            .await;

        let service = open_with_subscriptions(&server, &[DESTINATION]).await?;
        let listed = service.subscriptions().await?;
        let result = listed.first().expect("one destination").delete().await?;

        if status == 200 {
            assert!(matches!(result, ModificationResponse::Empty));
        } else {
            assert!(matches!(result, ModificationResponse::Task(_)));
        }
    }

    Ok(())
}

#[test]
async fn delete_does_not_hide_malformed_destination_fields() -> Result<(), Box<dyn StdError>> {
    for missing_context in [false, true] {
        let server = MockServer::start().await;
        let mut body = destination(DESTINATION, Some("SSE"));

        serve(&server, DESTINATION, &body).await;

        if missing_context {
            let object = body.as_object_mut().expect("destination");
            object.remove("SubscriptionType");
            object.remove("Context");
        } else {
            body["SubscriptionType"] = json!(42);
        }

        Mock::given(method("DELETE"))
            .and(path(DESTINATION))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&server)
            .await;

        let service = open_with_subscriptions(&server, &[DESTINATION]).await?;
        let listed = service.subscriptions().await?;
        let result = listed.first().expect("one destination").delete().await;

        assert!(matches!(result, Err(nv_redfish::Error::Decode(_))));
    }

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
