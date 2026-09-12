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

mod common;

#[cfg(feature = "reqwest")]
mod tests {
    use crate::common::test_utils::*;
    use futures_util::StreamExt;
    use nv_redfish_bmc_http::reqwest::BmcError;
    use nv_redfish_core::Bmc;
    use serde::Deserialize;
    use serde_json::Value as JsonValue;
    use wiremock::{
        matchers::{header, method, path},
        Match, Mock, MockServer, Request, ResponseTemplate,
    };

    const SSE_URI: &str = "/redfish/v1/EventService/SSE";

    #[derive(Debug, Deserialize, PartialEq)]
    struct StreamPayload {
        event_id: String,
        severity: String,
    }

    struct WithoutHeader(&'static str);

    impl Match for WithoutHeader {
        fn matches(&self, request: &Request) -> bool {
            !request.headers.contains_key(self.0)
        }
    }

    #[tokio::test]
    async fn test_event_stream_reads_typed_json() {
        let mock_server = MockServer::start().await;
        let sse_body = concat!(
            "event: Alert\n",
            "data: {\"event_id\":\"1\",\"severity\":\"Critical\"}\n\n",
            "event: StatusChange\n",
            "data: {\"event_id\":\"2\",\"severity\":\"OK\"}\n\n"
        );

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("accept", "text/event-stream"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let mut stream = bmc
            .stream::<JsonValue>(SSE_URI)
            .await
            .expect("must open stream");

        let first = stream
            .next()
            .await
            .expect("first event expected")
            .expect("first event parse");
        assert_eq!(
            first,
            serde_json::json!({
                "event_id": "1",
                "severity": "Critical"
            })
        );

        let second = stream
            .next()
            .await
            .expect("second event expected")
            .expect("second event parse");
        assert_eq!(
            second,
            serde_json::json!({
                "event_id": "2",
                "severity": "OK"
            })
        );

        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn test_event_stream_json_decodes_payload() {
        let mock_server = MockServer::start().await;
        let sse_body = concat!(
            "event: Alert\n",
            "data: {\"event_id\":\"10\",\"severity\":\"Warning\"}\n\n",
            "event: Alert\n",
            "data: {\"event_id\":\"11\",\"severity\":\"Critical\"}\n\n"
        );

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("accept", "text/event-stream"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let mut stream = bmc
            .stream::<StreamPayload>(SSE_URI)
            .await
            .expect("must open stream");

        let first = stream
            .next()
            .await
            .expect("first event expected")
            .expect("first event parse");
        assert_eq!(
            first,
            StreamPayload {
                event_id: "10".to_string(),
                severity: "Warning".to_string(),
            }
        );

        let second = stream
            .next()
            .await
            .expect("second event expected")
            .expect("second event parse");
        assert_eq!(
            second,
            StreamPayload {
                event_id: "11".to_string(),
                severity: "Critical".to_string(),
            }
        );

        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn sse_aborts_when_event_exceeds_byte_limit() {
        use nv_redfish_bmc_http::reqwest::{Client, ClientParams};
        use nv_redfish_bmc_http::{CacheSettings, HttpBmc};
        use url::Url;

        let mock_server = MockServer::start().await;

        // 6-byte prefix ("data: ") + 20 bytes of data + newline = 27 bytes total,
        // well over the 16-byte limit. No event terminator (\n\n) so the decoder
        // never emits a complete event — the counter fires first.
        let sse_body = format!("data: {}\n", "x".repeat(20));

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(header("accept", "text/event-stream"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(sse_body),
            )
            .mount(&mock_server)
            .await;

        let client = Client::with_params(ClientParams::new().sse_max_event_bytes(16)).unwrap();
        let bmc = HttpBmc::new(
            client,
            Url::parse(&mock_server.uri()).unwrap(),
            create_test_credentials(),
            CacheSettings::default(),
        );

        let mut stream = bmc
            .stream::<JsonValue>(SSE_URI)
            .await
            .expect("stream must open");

        let result = stream.next().await.expect("expected an error item");
        assert!(
            matches!(result, Err(BmcError::SseEventTooLarge { limit: 16 })),
            "expected SseEventTooLarge, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn sse_aborts_on_idle_timeout() {
        use nv_redfish_bmc_http::reqwest::{Client, ClientParams};
        use nv_redfish_bmc_http::{CacheSettings, HttpBmc};
        use std::time::Duration;
        use tokio::io::AsyncWriteExt as _;
        use tokio::net::TcpListener;
        use url::Url;

        // Bind to an OS-assigned port so we never collide with other tests.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Spawn a minimal HTTP server that sends one SSE event then stalls,
        // keeping the connection open so the client has nothing to time out on.
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\n\
                      Content-Type: text/event-stream\r\n\
                      Connection: keep-alive\r\n\
                      \r\n\
                      data: {}\n\n",
                )
                .await
                .unwrap();
            socket.flush().await.unwrap();
            // Hold the connection open so the idle timeout is what fires, not EOF.
            tokio::time::sleep(Duration::from_secs(30)).await;
        });

        let client =
            Client::with_params(ClientParams::new().sse_idle_timeout(Duration::from_millis(100)))
                .unwrap();
        let bmc = HttpBmc::new(
            client,
            Url::parse(&format!("http://{addr}")).unwrap(),
            create_test_credentials(),
            CacheSettings::default(),
        );

        let mut stream = bmc
            .stream::<JsonValue>(SSE_URI)
            .await
            .expect("stream must open");

        // First poll delivers the one event the server sent.
        let first = stream
            .next()
            .await
            .expect("first event expected")
            .expect("first event must be Ok");
        assert_eq!(first, serde_json::json!({}));

        // Second poll blocks until the idle timeout fires.
        let result = stream.next().await.expect("expected an error item");
        assert!(
            matches!(result, Err(BmcError::SseIdleTimeout { .. })),
            "expected SseIdleTimeout, got: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn test_event_stream_rejects_cross_origin_uri() {
        let mock_server = MockServer::start().await;
        let bmc = create_test_bmc(&mock_server);

        let result = bmc
            .stream::<JsonValue>("https://bmc.example.evil/redfish/v1/EventService/SSE")
            .await;

        assert!(matches!(result, Err(BmcError::InvalidRequest(_))));
    }

    fn sse_response(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .insert_header("content-type", "text/event-stream")
            .set_body_string(body)
    }

    #[tokio::test]
    async fn event_ids_are_carried_and_persist_until_the_server_changes_them() {
        let mock_server = MockServer::start().await;
        // The SSE processing model: an `id` field sets the last event id, a
        // frame without one keeps it, an id-only frame moves it without
        // dispatching an event, an `id` holding a NUL is ignored, and an
        // empty `id` clears it.
        let sse_body = concat!(
            "id: 7\n",
            "data: {\"n\":1}\n\n",
            "data: {\"n\":2}\n\n",
            "id: 9\n\n",
            "data: {\"n\":3}\n\n",
            "id: 9\u{0}x\n",
            "data: {\"n\":4}\n\n",
            "id: \n",
            "data: {\"n\":5}\n\n"
        );

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .respond_with(sse_response(sse_body))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let stream = bmc
            .stream_events::<JsonValue>(SSE_URI, None)
            .await
            .expect("must open stream");
        let events: Vec<_> = stream
            .map(|event| event.expect("event parse"))
            .collect()
            .await;

        let ids: Vec<Option<&str>> = events
            .iter()
            .map(|event| event.last_event_id.as_deref())
            .collect();
        assert_eq!(ids, [Some("7"), Some("7"), Some("9"), Some("9"), None]);
        let payloads: Vec<u64> = events
            .iter()
            .map(|event| event.data["n"].as_u64().expect("a number"))
            .collect();
        assert_eq!(payloads, [1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn the_plain_stream_is_the_same_events_without_their_ids() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .respond_with(sse_response(
                "id: 7\ndata: {\"n\":1}\n\ndata: {\"n\":2}\n\n",
            ))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let stream = bmc
            .stream::<JsonValue>(SSE_URI)
            .await
            .expect("must open stream");
        let payloads: Vec<JsonValue> = stream
            .map(|payload| payload.expect("event parse"))
            .collect()
            .await;

        assert_eq!(
            payloads,
            [serde_json::json!({"n": 1}), serde_json::json!({"n": 2})]
        );
    }

    #[tokio::test]
    async fn a_resumed_stream_sends_the_last_event_id() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(header("last-event-id", "7"))
            .respond_with(sse_response("id: 8\ndata: {}\n\n"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let mut stream = bmc
            .stream_events::<JsonValue>(SSE_URI, Some("7"))
            .await
            .expect("must open stream");

        let event = stream
            .next()
            .await
            .expect("one event")
            .expect("event parse");
        assert_eq!(event.last_event_id.as_deref(), Some("8"));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn the_resume_id_stays_in_effect_until_the_server_sets_another() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(header("last-event-id", "7"))
            .respond_with(sse_response(
                "data: {\"n\":1}\n\nid: 8\ndata: {\"n\":2}\n\n",
            ))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let stream = bmc
            .stream_events::<JsonValue>(SSE_URI, Some("7"))
            .await
            .expect("must open stream");
        let events: Vec<_> = stream
            .map(|event| event.expect("event parse"))
            .collect()
            .await;

        let ids: Vec<Option<&str>> = events
            .iter()
            .map(|event| event.last_event_id.as_deref())
            .collect();
        assert_eq!(ids, [Some("7"), Some("8")]);
    }

    #[tokio::test]
    async fn an_empty_resume_id_is_no_resume_id() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(WithoutHeader("last-event-id"))
            .respond_with(sse_response("data: {}\n\n"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let mut stream = bmc
            .stream_events::<JsonValue>(SSE_URI, Some(""))
            .await
            .expect("must open stream");
        let event = stream
            .next()
            .await
            .expect("one event")
            .expect("event parse");
        assert_eq!(event.last_event_id, None);
    }

    #[tokio::test]
    async fn a_fresh_stream_sends_no_last_event_id() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .and(WithoutHeader("last-event-id"))
            .respond_with(sse_response("data: {}\n\n"))
            .expect(2)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let mut stream = bmc
            .stream_events::<JsonValue>(SSE_URI, None)
            .await
            .expect("must open stream");
        let event = stream
            .next()
            .await
            .expect("one event")
            .expect("event parse");
        assert_eq!(event.last_event_id, None);

        let mut stream = bmc
            .stream::<JsonValue>(SSE_URI)
            .await
            .expect("must open stream");
        assert!(stream.next().await.is_some());
    }

    #[tokio::test]
    async fn a_last_event_id_that_is_not_a_header_value_is_rejected_before_transport() {
        let mock_server = MockServer::start().await;
        let bmc = create_test_bmc(&mock_server);

        let result = bmc.stream_events::<JsonValue>(SSE_URI, Some("7\n8")).await;

        assert!(matches!(result, Err(BmcError::InvalidRequest(_))));
    }
}
