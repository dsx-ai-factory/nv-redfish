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

#[cfg(feature = "reqwest-client")]
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

    /// Verifies that the SSE stream aborts when an event exceeds `sse_max_event_bytes`.
    #[tokio::test]
    async fn sse_aborts_when_event_exceeds_byte_limit() {
        use nv_redfish_bmc_http::reqwest::ClientParams;
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

        let client = test_client(ClientParams::new().sse_max_event_bytes(16));
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

        assert!(stream.next().await.is_none());
    }

    /// Verifies that the SSE stream aborts when no frame arrives within `sse_idle_timeout`.
    #[tokio::test]
    async fn sse_aborts_on_idle_timeout() {
        use nv_redfish_bmc_http::reqwest::ClientParams;
        use nv_redfish_bmc_http::{CacheSettings, HttpBmc};
        use std::time::Duration;
        use tokio::io::AsyncReadExt as _;
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
            // Read the whole request first: a response that arrives while the
            // client is still sending it makes the client cancel the request.
            let mut request = Vec::new();
            let mut buf = [0u8; 1024];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "connection closed before the request ended");
                request.extend_from_slice(&buf[..n]);
            }
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

        let client = test_client(ClientParams::new().sse_idle_timeout(Duration::from_millis(100)));
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

        assert!(stream.next().await.is_none());
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
    async fn valid_sse_fields_preserve_data_and_update_the_resume_cursor() {
        for (fields, expected_id) in [
            ("extension: ignored\n", Some("previous")),
            ("id: first\nid: second\n", Some("second")),
            ("id\n", None),
            ("id: valid\nid: ignored\0value\n", Some("valid")),
        ] {
            for line_ending in ["\n", "\r\n", "\r"] {
                let mock_server = MockServer::start().await;
                let body = format!("{fields}data: 1\n\ndata: 2\n\n").replace('\n', line_ending);

                Mock::given(method("GET"))
                    .and(path(SSE_URI))
                    .respond_with(sse_response(&body))
                    .expect(1)
                    .mount(&mock_server)
                    .await;

                let bmc = create_test_bmc(&mock_server);

                let mut stream = bmc
                    .stream_events::<u64>(SSE_URI, Some("previous"))
                    .await
                    .expect("stream must open");

                for expected_data in [1, 2] {
                    let event = stream
                        .next()
                        .await
                        .expect("event must arrive")
                        .expect("valid SSE fields must decode");

                    assert_eq!(event.data, expected_data, "fields: {fields:?}");
                    assert_eq!(event.last_event_id.as_deref(), expected_id);
                }

                assert!(stream.next().await.is_none());
            }
        }
    }

    #[tokio::test]
    async fn an_empty_data_field_returns_a_json_error_without_ending_the_stream() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .respond_with(sse_response("id: first\ndata\n\ndata: 2\n\n"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let mut stream = bmc
            .stream_events::<u64>(SSE_URI, None)
            .await
            .expect("stream must open");

        let first = stream.next().await.expect("JSON error must arrive");

        assert!(matches!(first, Err(BmcError::JsonError(_))));

        let second = stream
            .next()
            .await
            .expect("subsequent event must arrive")
            .expect("subsequent event must decode");

        assert_eq!(second.data, 2);
        assert_eq!(second.last_event_id.as_deref(), Some("first"));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn invalid_utf8_ends_the_stream_after_one_decoder_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path(SSE_URI))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_bytes(b"data: \xff\n\ndata: 2\n\n".to_vec()),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let mut stream = bmc.stream::<u64>(SSE_URI).await.expect("stream must open");
        let first = stream.next().await.expect("decoder error must arrive");

        assert!(matches!(first, Err(BmcError::SseStreamError(_))));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn fragmented_multiline_sse_survives_until_a_transport_error() {
        use nv_redfish_bmc_http::reqwest::ClientParams;
        use nv_redfish_bmc_http::{CacheSettings, HttpBmc};

        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        use tokio::net::TcpListener;
        use url::Url;

        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback server");

        let address = listener.local_addr().expect("loopback address");

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("client connection");

            let mut request = Vec::new();
            let mut buffer = [0_u8; 1024];

            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).await.expect("request headers");

                assert!(count > 0, "request must finish before the client closes");

                request.extend_from_slice(&buffer[..count]);
            }

            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                      Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
                )
                .await
                .expect("response headers");

            // Split fields and CRLF pairs across HTTP chunks. Closing without
            // a final zero-length chunk then produces a transport body error.
            for chunk in [
                "id: fir",
                "st\r",
                "\ndata: {\"n\":\r\nda",
                "ta: 1}\r",
                "\n\r",
                "\ndata: {",
            ] {
                let framed = format!("{:x}\r\n{chunk}\r\n", chunk.len());

                socket
                    .write_all(framed.as_bytes())
                    .await
                    .expect("HTTP chunk");
            }
        });

        let bmc = HttpBmc::new(
            test_client(ClientParams::new()),
            Url::parse(&format!("http://{address}")).expect("loopback URL"),
            create_test_credentials(),
            CacheSettings::default(),
        );

        let mut stream = bmc
            .stream_events::<JsonValue>(SSE_URI, None)
            .await
            .expect("stream must open");

        let first = stream
            .next()
            .await
            .expect("complete event must arrive")
            .expect("fragmented multiline JSON must decode");

        assert_eq!(first.data, serde_json::json!({"n": 1}));
        assert_eq!(first.last_event_id.as_deref(), Some("first"));

        let error = stream.next().await.expect("transport error must arrive");

        assert!(matches!(error, Err(BmcError::ReqwestError(_))));
        assert!(stream.next().await.is_none());

        server.await.expect("loopback server must finish");
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
