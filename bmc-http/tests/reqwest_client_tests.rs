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
mod reqwest_client_tests {
    #[cfg(feature = "http-extras")]
    use std::num::NonZeroUsize;
    use std::time::Duration;

    use futures_util::io::Cursor;
    use nv_redfish_bmc_http::reqwest::BmcError;
    use nv_redfish_bmc_http::reqwest::Client;
    use nv_redfish_bmc_http::reqwest::ClientParams;
    use nv_redfish_bmc_http::reqwest::RetryPolicy;
    #[cfg(any(feature = "http-extras", feature = "update-service-deprecated"))]
    use nv_redfish_bmc_http::BmcCredentials;
    use nv_redfish_bmc_http::CacheSettings;
    use nv_redfish_bmc_http::HttpBmc;
    use nv_redfish_bmc_http::HttpClient;
    #[cfg(feature = "update-service-deprecated")]
    use nv_redfish_core::HttpPushUriUpdateRequest;
    #[cfg(feature = "update-service-deprecated")]
    use nv_redfish_core::UploadStream;
    use nv_redfish_core::{
        query::{ExpandQuery, FilterQuery},
        Action, Bmc, DataStream, ModificationResponse, MultipartUpdateRequest, ODataId,
        OperationResponseBmc,
    };
    use serde::{Deserialize, Serialize};
    use url::Url;
    #[cfg(feature = "update-service-deprecated")]
    use wiremock::Request;
    use wiremock::{
        matchers::{body_json, header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    use crate::common::test_utils::*;

    struct FailingUpdateParameters;

    #[derive(Deserialize)]
    struct LocationResource {
        #[serde(rename = "@odata.id")]
        odata_id: ODataId,
    }

    impl Serialize for FailingUpdateParameters {
        fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("serialize failed"))
        }
    }

    #[tokio::test]
    async fn test_get_request_success() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;

        let test_resource =
            create_test_resource(resource_path, Some("123"), names::TEST_SYSTEM, 42);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.get::<TestResource>(&resource_id).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.name, names::TEST_SYSTEM);
        assert_eq!(retrieved.value, 42);
    }

    /// Builds a retry policy through the public API only, the way a
    /// downstream crate without its own reqwest dependency would.
    #[tokio::test]
    async fn test_retry_policy_via_public_api() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;

        let test_resource =
            create_test_resource(resource_path, Some("123"), names::TEST_SYSTEM, 42);

        // The first request is rejected, the retry succeeds.
        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let policy = RetryPolicy::new(|_request, response| {
            response.status() == http::StatusCode::SERVICE_UNAVAILABLE
        })
        .max_retries(1)
        .delay(Duration::from_millis(10));

        let client = Client::with_params(ClientParams::new().retry(policy))?;
        let bmc = HttpBmc::new(
            client,
            Url::parse(&mock_server.uri())?,
            create_test_credentials(),
            CacheSettings::default(),
        );

        let resource_id = create_odata_id(resource_path);
        let retrieved = bmc.get::<TestResource>(&resource_id).await?;

        assert_eq!(retrieved.name, names::TEST_SYSTEM);
        assert_eq!(retrieved.value, 42);

        Ok(())
    }

    #[cfg(feature = "http-extras")]
    #[tokio::test]
    async fn concurrency_limited_bmc_preserves_credential_rotation() {
        let mock_server = MockServer::start().await;
        let first_resource_path = paths::SYSTEMS_1;
        let second_resource_path = paths::MANAGERS_1;

        let first_resource =
            create_test_resource(first_resource_path, Some("123"), names::TEST_SYSTEM, 42);
        let second_resource =
            create_test_resource(second_resource_path, Some("456"), names::TEST_MANAGER, 7);

        Mock::given(method("GET"))
            .and(path(first_resource_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&first_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(second_resource_path))
            .and(header("X-Auth-Token", "new-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&second_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server).with_request_concurrency_limit(NonZeroUsize::MIN);

        let first_id = create_odata_id(first_resource_path);
        let first = bmc.get::<TestResource>(&first_id).await.unwrap();
        assert_eq!(first.value, 42);

        bmc.set_credentials(BmcCredentials::token("new-token".to_string()));

        let second_id = create_odata_id(second_resource_path);
        let second = bmc.get::<TestResource>(&second_id).await.unwrap();
        assert_eq!(second.value, 7);
    }

    #[tokio::test]
    async fn test_get_request_with_expand() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;

        let test_resource =
            create_test_resource(resource_path, Some("456"), names::TEST_SYSTEM, 100);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(query_param("$skiptoken", "abc"))
            .and(query_param("$expand", ".($levels=2)"))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(&format!("{resource_path}?$skiptoken=abc"));
        let expand_query = ExpandQuery::current().levels(2);
        let result = bmc.expand::<TestResource>(&resource_id, expand_query).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.name, names::TEST_SYSTEM);
        assert_eq!(retrieved.value, 100);
    }

    #[tokio::test]
    async fn test_get_request_with_filter() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;

        let test_resource =
            create_test_resource(resource_path, Some("789"), names::TEST_SYSTEM, 50);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(query_param("$skiptoken", "abc"))
            .and(query_param("$filter", "value gt 10"))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(&format!("{resource_path}?$skiptoken=abc"));
        let filter_query = FilterQuery::gt(&"value", 10);
        let result = bmc.filter::<TestResource>(&resource_id, filter_query).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.name, names::TEST_SYSTEM);
        assert_eq!(retrieved.value, 50);
    }

    #[tokio::test]
    async fn body_bearing_create_response_ignores_invalid_location(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let collection_path = paths::SYSTEMS_1;

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };

        let created_resource =
            create_test_resource("/redfish/v1/systems/new", None, names::TEST_SYSTEM, 999);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("Location", "#fragment")
                    .set_body_json(&created_resource),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let collection_id = create_odata_id(collection_path);
        let response = bmc
            .create::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await?;

        let ModificationResponse::Entity(created) = response else {
            return Err(String::from("expected entity response").into());
        };

        assert_eq!(created.name, names::TEST_SYSTEM);
        assert_eq!(created.value, 999);

        Ok(())
    }

    #[tokio::test]
    async fn relative_session_location_with_query_is_used_for_delete(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/SessionService/Sessions";
        let session_path = "/redfish/v1/SessionService/Sessions/1";
        let expected_location = format!("{session_path}?session=abc");

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };

        let created_resource = create_test_resource(session_path, None, names::TEST_SYSTEM, 999);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("X-Auth-Token", "session-token-123")
                    .insert_header("Location", "Sessions/1?session=abc")
                    .set_body_json(&created_resource),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path(session_path))
            .and(query_param("session", "abc"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let collection_id = create_odata_id(collection_path);

        let response = bmc
            .create_session::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await?;

        assert_eq!(response.auth_token, "session-token-123");
        assert_eq!(response.location.to_string(), expected_location);
        assert_eq!(response.entity.name, names::TEST_SYSTEM);
        assert_eq!(response.entity.value, 999);

        let deleted = bmc.delete::<TestResource>(&response.location).await?;

        assert!(matches!(deleted, ModificationResponse::Empty));
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn absolute_session_location_is_used_for_delete() -> Result<(), Box<dyn std::error::Error>>
    {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/SessionService/Sessions";
        let session_path = "/redfish/v1/SessionService/Sessions/2";
        let absolute_location = format!("{}{session_path}", mock_server.uri());

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 1000,
        };

        let created_resource = create_test_resource(session_path, None, names::TEST_SYSTEM, 1000);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("X-Auth-Token", "session-token-456")
                    .insert_header("Location", absolute_location)
                    .set_body_json(&created_resource),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("DELETE"))
            .and(path(session_path))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let collection_id = create_odata_id(collection_path);

        let response = bmc
            .create_session::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await?;

        assert_eq!(response.auth_token, "session-token-456");
        assert_eq!(response.location.to_string(), session_path);
        assert_eq!(response.entity.value, 1000);

        let deleted = bmc.delete::<TestResource>(&response.location).await?;

        assert!(matches!(deleted, ModificationResponse::Empty));
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn relative_task_location_with_query_can_be_polled(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/Systems";
        let task_path = "/redfish/v1/TaskService/Tasks/42";
        let expected_location = format!("{task_path}?monitor=abc");

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };

        let task_resource = create_test_resource(task_path, None, "Update task", 50);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("Location", "TaskService/Tasks/42?monitor=abc"),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(task_path))
            .and(query_param("monitor", "abc"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&task_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let collection_id = create_odata_id(collection_path);

        let response = bmc
            .create::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await?;

        let ModificationResponse::Task(task) = response else {
            return Err(String::from("expected task response").into());
        };

        assert_eq!(task.location.0.to_string(), expected_location);

        let task = bmc.get::<TestResource>(&task.location.0).await?;

        assert_eq!(task.name, "Update task");
        assert_eq!(task.value, 50);
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn async_operation_rejects_cross_origin_location(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/Systems";

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .respond_with(ResponseTemplate::new(202).insert_header(
                "Location",
                "https://other.example/redfish/v1/TaskService/Tasks/42",
            ))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let collection_id = create_odata_id(collection_path);

        let result = bmc
            .create::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await;

        let Err(BmcError::InvalidResponse { status, text, .. }) = result else {
            return Err(String::from("expected invalid response error").into());
        };

        assert_eq!(status, reqwest::StatusCode::ACCEPTED);
        assert_eq!(text, "Location header resolves to a different origin");

        Ok(())
    }

    #[tokio::test]
    async fn test_multipart_update_reports_encode_errors() -> Result<(), Box<dyn std::error::Error>>
    {
        let bmc = HttpBmc::new(
            Client::new()?,
            Url::parse("http://127.0.0.1")?,
            create_test_credentials(),
            CacheSettings::default(),
        );

        let request = MultipartUpdateRequest {
            update_parameters: &FailingUpdateParameters,
            update_stream: DataStream::new("firmware.bin", Cursor::new(Vec::<u8>::new())),
            oem_parts: Vec::new(),
            upload_timeout: Duration::from_secs(600),
        };

        let result = bmc
            .multipart_update::<_, _, TestResource>(
                "/redfish/v1/UpdateService/update-multipart",
                request,
            )
            .await;

        assert!(matches!(result, Err(BmcError::EncodeError(_))));

        Ok(())
    }

    #[tokio::test]
    async fn multipart_update_rejects_cross_origin_uri() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let bmc = create_test_bmc(&mock_server);

        let request = MultipartUpdateRequest {
            update_parameters: &(),
            update_stream: DataStream::new("firmware.bin", Cursor::new(Vec::<u8>::new())),
            oem_parts: Vec::new(),
            upload_timeout: Duration::from_secs(600),
        };

        let result = bmc
            .multipart_update::<_, _, TestResource>(
                "https://bmc.example.evil/redfish/v1/UpdateService/upload",
                request,
            )
            .await;

        assert!(matches!(result, Err(BmcError::InvalidRequest(_))));

        Ok(())
    }

    #[cfg(feature = "update-service-deprecated")]
    #[tokio::test]
    async fn http_push_uri_relative_task() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update";
        let task_path = "/redfish/v1/TaskService/Tasks/42";

        Mock::given(method("POST"))
            .and(path(upload_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("X-Upload-Mode", "raw"))
            .and(header("content-type", "application/octet-stream"))
            .and(header("content-length", "14"))
            .and(|request: &Request| request.body == b"firmware-bytes")
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("Location", format!("{}{task_path}", mock_server.uri()))
                    .insert_header("Retry-After", "15"),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut custom_headers = http::HeaderMap::new();
        custom_headers.insert("X-Upload-Mode", http::HeaderValue::from_static("raw"));

        let bmc = create_test_bmc_with_custom_headers(&mock_server, custom_headers);
        let request = HttpPushUriUpdateRequest {
            update_stream: UploadStream::new(Cursor::new(b"firmware-bytes".to_vec()))
                .with_content_length(14),
            upload_timeout: Duration::from_secs(600),
        };

        let response = bmc
            .http_push_uri_update::<_, TestResource>(upload_path, request)
            .await?;

        let ModificationResponse::Task(task) = response else {
            return Err(String::from("expected task response").into());
        };

        assert_eq!(task.location.0.to_string(), task_path);
        assert_eq!(task.retry_after, Some(Duration::from_secs(15)));

        Ok(())
    }

    #[cfg(feature = "update-service-deprecated")]
    #[tokio::test]
    async fn http_push_uri_absolute_token_empty() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update";

        Mock::given(method("POST"))
            .and(path(upload_path))
            .and(header("X-Auth-Token", "session-token"))
            .and(header("content-type", "application/octet-stream"))
            .and(|request: &Request| !request.headers.contains_key("content-length"))
            .and(|request: &Request| request.body == b"firmware-bytes")
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc_with_credentials(
            &mock_server,
            BmcCredentials::token("session-token".to_string()),
        );
        let request = HttpPushUriUpdateRequest {
            update_stream: UploadStream::new(Cursor::new(b"firmware-bytes".to_vec())),
            upload_timeout: Duration::from_secs(600),
        };
        let upload_url = format!("{}{upload_path}", mock_server.uri());

        let response = bmc
            .http_push_uri_update::<_, ()>(&upload_url, request)
            .await?;

        assert!(matches!(response, ModificationResponse::Empty));

        Ok(())
    }

    #[cfg(feature = "update-service-deprecated")]
    #[tokio::test]
    async fn http_push_uri_rejects_cross_origin_uri() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let bmc = create_test_bmc(&mock_server);

        let request = HttpPushUriUpdateRequest {
            update_stream: UploadStream::new(Cursor::new(b"firmware-bytes".to_vec())),
            upload_timeout: Duration::from_secs(600),
        };

        let result = bmc
            .http_push_uri_update::<_, ()>(
                "https://bmc.example.evil/redfish/v1/UpdateService/update",
                request,
            )
            .await;

        assert!(matches!(result, Err(BmcError::InvalidRequest(_))));

        Ok(())
    }

    #[cfg(feature = "update-service-deprecated")]
    #[tokio::test]
    async fn http_push_uri_error_response() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update";

        Mock::given(method("POST"))
            .and(path(upload_path))
            .respond_with(ResponseTemplate::new(500).set_body_string("upload failed"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let request = HttpPushUriUpdateRequest {
            update_stream: UploadStream::new(Cursor::new(b"firmware-bytes".to_vec()))
                .with_content_length(14),
            upload_timeout: Duration::from_secs(600),
        };

        let result = bmc
            .http_push_uri_update::<_, ()>(upload_path, request)
            .await;

        assert!(matches!(
            result,
            Err(BmcError::InvalidResponse { status, .. }) if status.as_u16() == 500
        ));

        Ok(())
    }

    #[cfg(feature = "update-service-deprecated")]
    #[tokio::test]
    async fn http_push_uri_timeout_scoped() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update";
        let resource_path = paths::SYSTEMS_1;
        let delayed_response = Duration::from_millis(200);

        Mock::given(method("POST"))
            .and(path(upload_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(|request: &Request| request.body == b"firmware-bytes")
            .respond_with(ResponseTemplate::new(204).set_delay(delayed_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_delay(delayed_response)
                    .set_body_json(create_test_resource(
                        resource_path,
                        None,
                        names::TEST_SYSTEM,
                        42,
                    )),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = Client::with_params(ClientParams::new().timeout(Duration::from_millis(50)))?;
        let bmc = HttpBmc::new(
            client,
            Url::parse(&mock_server.uri())?,
            create_test_credentials(),
            CacheSettings::default(),
        );
        let request = HttpPushUriUpdateRequest {
            update_stream: UploadStream::new(Cursor::new(b"firmware-bytes".to_vec())),
            upload_timeout: Duration::from_secs(2),
        };

        let upload = bmc
            .http_push_uri_update::<_, ()>(upload_path, request)
            .await?;

        assert!(matches!(upload, ModificationResponse::Empty));

        let resource_id = create_odata_id(resource_path);
        let error = bmc
            .get::<TestResource>(&resource_id)
            .await
            .expect_err("expected default GET timeout");

        let BmcError::ReqwestError(err) = error else {
            return Err(String::from("expected default GET timeout").into());
        };

        assert!(err.is_timeout());

        Ok(())
    }

    #[tokio::test]
    async fn test_create_session_missing_token_is_error() {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/SessionService/Sessions";
        let session_path = "/redfish/v1/SessionService/Sessions/1";

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };
        let created_resource = create_test_resource(session_path, None, names::TEST_SYSTEM, 999);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("Location", session_path)
                    .set_body_json(&created_resource),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let collection_id = create_odata_id(collection_path);
        let error = bmc
            .create_session::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await
            .unwrap_err();

        assert!(matches!(error, BmcError::InvalidResponse { .. }));
    }

    #[tokio::test]
    async fn test_create_session_missing_location_is_error() {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/SessionService/Sessions";
        let session_path = "/redfish/v1/SessionService/Sessions/1";

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };
        let created_resource = create_test_resource(session_path, None, names::TEST_SYSTEM, 999);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .respond_with(
                ResponseTemplate::new(201)
                    .insert_header("X-Auth-Token", "session-token-123")
                    .set_body_json(&created_resource),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let collection_id = create_odata_id(collection_path);
        let error = bmc
            .create_session::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await
            .unwrap_err();

        assert!(matches!(error, BmcError::InvalidResponse { .. }));
    }

    #[tokio::test]
    async fn test_patch_update_request() {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1/systems/1";

        let update_request = UpdateRequest {
            name: Some("Updated System".to_string()),
            value: None,
        };

        let etag = create_odata_etag("abc123");

        let updated_resource = TestResource {
            id: create_odata_id(resource_path),
            etag: None,
            name: "Updated System".to_string(),
            value: 42,
        };

        Mock::given(method("PATCH"))
            .and(path(resource_path))
            .and(body_json(&update_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("If-Match", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&updated_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("PATCH"))
            .and(path(resource_path))
            .and(body_json(&update_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("If-Match", "*"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&updated_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let result = bmc
            .update::<UpdateRequest, TestResource>(&resource_id, Some(&etag), &update_request)
            .await;

        assert!(result.is_ok());
        let updated = match result.unwrap() {
            ModificationResponse::Entity(updated) => updated,
            _ => panic!("expected entity response"),
        };
        assert_eq!(updated.name, "Updated System");
        assert_eq!(updated.value, 42);

        let no_etag = bmc
            .update::<UpdateRequest, TestResource>(&resource_id, None, &update_request)
            .await;

        assert!(no_etag.is_ok());
    }

    #[tokio::test]
    async fn test_http_patch_returns_typed_body_without_odata_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let endpoint_path = "/redfish/v1/Oem/Nvidia/TypedPatch";

        let request = UpdateRequest {
            name: Some("Updated System".to_string()),
            value: None,
        };

        let typed_response = ActionResponse {
            result: "patched".to_string(),
            success: true,
        };

        Mock::given(method("PATCH"))
            .and(path(endpoint_path))
            .and(body_json(&request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("If-Match", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&typed_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = Client::new()?;
        let credentials = create_test_credentials();
        let custom_headers = http::HeaderMap::new();
        let response = client
            .patch::<UpdateRequest, ActionResponse>(
                Url::parse(&format!("{}{endpoint_path}", mock_server.uri()))?,
                create_odata_etag("abc123"),
                &request,
                &credentials,
                &custom_headers,
            )
            .await?;

        let ModificationResponse::Entity(body) = response else {
            return Err(String::from("expected typed response body").into());
        };

        assert_eq!(body.result, "patched");
        assert!(body.success);

        Ok(())
    }

    #[tokio::test]
    async fn no_content_delete_response_ignores_invalid_location(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1/systems/1";

        Mock::given(method("DELETE"))
            .and(path(resource_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(204).insert_header("Location", "#fragment"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let response = bmc.delete::<TestResource>(&resource_id).await?;

        assert!(matches!(response, ModificationResponse::Empty));

        Ok(())
    }

    #[tokio::test]
    async fn test_action_request() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/systems/1/Actions/ComputerSystem.Reset";

        let action_request = ActionRequest {
            parameter: "ForceRestart".to_string(),
        };

        let action_response = ActionResponse {
            result: "Reset initiated".to_string(),
            success: true,
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&action_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let action = create_test_action(action_path);
        let response = bmc.action(&action, &action_request).await?;

        let ModificationResponse::Entity(action_response) = response else {
            return Err(String::from("expected typed response body").into());
        };

        assert_eq!(action_response.result, "Reset initiated");
        assert!(action_response.success);

        Ok(())
    }

    #[tokio::test]
    async fn test_action_body_task_is_normalized_for_200_and_201(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for status in [200, 201] {
            let mock_server = MockServer::start().await;
            let action_path = "/redfish/v1/ComponentIntegrity/1/Actions/GetMeasurements";
            let task_path = format!("/redfish/v1/TaskService/Tasks/body-{status}");
            let monitor_path = format!("/redfish/v1/TaskService/TaskMonitors/{status}");
            let action_request = ActionRequest {
                parameter: "Nonce".to_string(),
            };

            Mock::given(method("POST"))
                .and(path(action_path))
                .and(body_json(&action_request))
                .respond_with(
                    ResponseTemplate::new(status)
                        .insert_header("Location", &monitor_path)
                        .insert_header("Retry-After", "4")
                        .set_body_json(serde_json::json!({
                            "@odata.id": task_path,
                            "@odata.type": "#Task.v1_4_3.Task",
                            "Id": status.to_string()
                        })),
                )
                .expect(1)
                .mount(&mock_server)
                .await;

            let bmc = create_test_bmc(&mock_server);
            let action: Action<ActionRequest, serde_json::Value> =
                serde_json::from_value(serde_json::json!({"target": action_path}))?;
            let response = bmc.action(&action, &action_request).await?;
            let ModificationResponse::Task(task) = response else {
                return Err(format!("expected task response for HTTP {status}").into());
            };
            assert_eq!(task.location.0.to_string(), monitor_path);
            assert_eq!(task.retry_after, Some(Duration::from_secs(4)));
            mock_server.verify().await;
        }

        Ok(())
    }

    #[tokio::test]
    async fn test_action_task_body_without_location_uses_odata_id(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/ComponentIntegrity/1/Actions/GetMeasurements";
        let task_path = "/redfish/v1/TaskService/Tasks/42";
        let action_request = ActionRequest {
            parameter: "Nonce".to_string(),
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@odata.id": task_path,
                "@odata.type": "#Task.v1_4_3.Task",
                "Id": "42"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let action = create_test_action(action_path);
        let response = bmc.action(&action, &action_request).await?;
        let ModificationResponse::Task(task) = response else {
            return Err("expected task response".into());
        };
        assert_eq!(task.location.0.to_string(), task_path);
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_action_body_under_task_path_without_task_type_is_entity(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/TaskService/Tasks/42/Actions/Oem.Refresh";
        let body = serde_json::json!({
            "@odata.id": "/redfish/v1/TaskService/Tasks/42/Oem/Detail",
            "Detail": "refreshed"
        });
        let action_request = ActionRequest {
            parameter: "Nonce".to_string(),
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .respond_with(ResponseTemplate::new(200).set_body_json(&body))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let action: Action<ActionRequest, serde_json::Value> =
            serde_json::from_value(serde_json::json!({"target": action_path}))?;
        let response = bmc.action(&action, &action_request).await?;
        let ModificationResponse::Entity(entity) = response else {
            return Err("expected entity response".into());
        };
        assert_eq!(entity, body);
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_action_202_task_body_without_location_remains_invalid() {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/ComponentIntegrity/1/Actions/GetMeasurements";
        let action_request = ActionRequest {
            parameter: "Nonce".to_string(),
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .respond_with(ResponseTemplate::new(202).set_body_json(serde_json::json!({
                "@odata.id": "/redfish/v1/TaskService/Tasks/42",
                "@odata.type": "#Task.v1_4_3.Task",
                "Id": "42"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let action = create_test_action(action_path);
        let error = bmc
            .action(&action, &action_request)
            .await
            .expect_err("202 without Location must be rejected");
        let BmcError::InvalidResponse { status, text, .. } = error else {
            panic!("unexpected error: {}", error);
        };
        assert_eq!(status, reqwest::StatusCode::ACCEPTED);
        assert_eq!(text, "202 Accepted without Location header");
        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_task_monitor_202_is_pending() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let monitor_path = "/redfish/v1/TaskService/TaskMonitors/HGX_0";
        let next_monitor_path = "/redfish/v1/TaskService/TaskMonitors/HGX_1";

        Mock::given(method("GET"))
            .and(path(monitor_path))
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("Location", next_monitor_path)
                    .insert_header("Retry-After", "30")
                    .set_body_json(serde_json::json!({
                        "@odata.id": "/redfish/v1/TaskService/Tasks/HGX_0",
                        "@odata.type": "#Task.v1_4_3.Task",
                        "Id": "HGX_0",
                        "TaskState": "Running"
                    })),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(monitor_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        let ModificationResponse::Task(task) = response else {
            return Err("expected pending task monitor".into());
        };
        assert_eq!(task.location.0.to_string(), next_monitor_path);
        assert_eq!(task.retry_after, Some(Duration::from_secs(30)));

        Ok(())
    }

    #[tokio::test]
    async fn test_task_monitor_200_returns_typed_result() -> Result<(), Box<dyn std::error::Error>>
    {
        let mock_server = MockServer::start().await;
        let monitor_path = "/redfish/v1/TaskService/TaskMonitors/HGX_0";

        Mock::given(method("GET"))
            .and(path(monitor_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"result": "measurements"})),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(monitor_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        let ModificationResponse::Entity(result) = response else {
            return Err("expected typed task monitor result".into());
        };
        assert_eq!(result, serde_json::json!({"result": "measurements"}));

        Ok(())
    }

    #[tokio::test]
    async fn test_task_monitor_204_returns_empty() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let monitor_path = "/redfish/v1/TaskService/TaskMonitors/HGX_0";

        Mock::given(method("GET"))
            .and(path(monitor_path))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(monitor_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        assert!(matches!(response, ModificationResponse::Empty));

        Ok(())
    }

    #[tokio::test]
    async fn test_task_monitor_202_without_location_keeps_polling_monitor(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let monitor_path = "/redfish/v1/TaskService/TaskMonitors/HGX_0";

        Mock::given(method("GET"))
            .and(path(monitor_path))
            .respond_with(ResponseTemplate::new(202).insert_header("Retry-After", "5"))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(monitor_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        let ModificationResponse::Task(task) = response else {
            return Err("expected pending task monitor".into());
        };
        assert_eq!(task.location.0.to_string(), monitor_path);
        assert_eq!(task.retry_after, Some(Duration::from_secs(5)));
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_task_monitor_204_follows_result_location(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let monitor_path = "/redfish/v1/TaskService/Tasks/HGX_0/Monitor";
        let result_path =
            "/redfish/v1/ComponentIntegrity/HGX_IRoT_GPU_0/Actions/SPDMGetSignedMeasurements/data";

        Mock::given(method("GET"))
            .and(path(monitor_path))
            .respond_with(ResponseTemplate::new(204).insert_header("Location", result_path))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path(result_path))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": "data"})),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(monitor_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        let ModificationResponse::Entity(result) = response else {
            return Err("expected result from the followed Location".into());
        };
        assert_eq!(result, serde_json::json!({"result": "data"}));
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_completed_task_follows_payload_location() -> Result<(), Box<dyn std::error::Error>>
    {
        let mock_server = MockServer::start().await;
        let task_path = "/redfish/v1/TaskService/Tasks/92";
        let result_path =
            "/redfish/v1/ComponentIntegrity/EROT_BIOS_0/Actions/SPDMGetSignedMeasurements/Data";

        Mock::given(method("GET"))
            .and(path(task_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@odata.id": task_path,
                "@odata.type": "#Task.v1_4_2.Task",
                "Id": "92",
                "Payload": {
                    "HttpHeaders": [format!("Location: {result_path}")]
                },
                "PercentComplete": 100,
                "TaskState": "Completed",
                "TaskStatus": "OK"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path(result_path))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"result": "viking"})),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(task_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        let ModificationResponse::Entity(result) = response else {
            return Err("expected result from the Task payload Location".into());
        };
        assert_eq!(result, serde_json::json!({"result": "viking"}));
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_task_resource_200_without_location_is_pending_at_task(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let task_path = "/redfish/v1/TaskService/Tasks/89";

        Mock::given(method("GET"))
            .and(path(task_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@odata.id": task_path,
                "@odata.type": "#Task.v1_4_3.Task",
                "Id": "89",
                "TaskState": "Running"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(task_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        let ModificationResponse::Task(task) = response else {
            return Err("expected pending Task resource".into());
        };
        assert_eq!(task.location.0.to_string(), task_path);
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_completed_task_resource_is_empty() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let task_path = "/redfish/v1/TaskService/Tasks/89";

        Mock::given(method("GET"))
            .and(path(task_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@odata.id": task_path,
                "@odata.type": "#Task.v1_4_3.Task",
                "Id": "89",
                "TaskState": "Completed"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(task_path.to_string());
        let response = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await?;
        assert!(matches!(response, ModificationResponse::Empty));
        mock_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn test_failed_task_resource_is_error() {
        let mock_server = MockServer::start().await;
        let task_path = "/redfish/v1/TaskService/Tasks/89";

        Mock::given(method("GET"))
            .and(path(task_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "@odata.id": task_path,
                "@odata.type": "#Task.v1_4_3.Task",
                "Id": "89",
                "TaskState": "Exception"
            })))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let location = ODataId::from(task_path.to_string());
        let error = bmc
            .get_operation_response::<serde_json::Value>(&location)
            .await
            .expect_err("failed Task must be an error");
        let BmcError::InvalidResponse { text, .. } = error else {
            panic!("unexpected error: {}", error);
        };
        assert_eq!(text, "Task ended in state Exception");
        mock_server.verify().await;
    }

    #[tokio::test]
    async fn test_action_request_absolute_target() -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/systems/1/Actions/ComputerSystem.Reset";
        let action_url = format!("{}{action_path}", mock_server.uri());

        let action_request = ActionRequest {
            parameter: "ForceRestart".to_string(),
        };

        let action_response = ActionResponse {
            result: "Reset initiated".to_string(),
            success: true,
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200).set_body_json(&action_response))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let action = create_test_action(&action_url);
        let response = bmc.action(&action, &action_request).await?;

        let ModificationResponse::Entity(action_response) = response else {
            return Err(String::from("expected typed response body").into());
        };

        assert_eq!(action_response.result, "Reset initiated");
        assert!(action_response.success);

        Ok(())
    }

    #[tokio::test]
    async fn test_action_request_rejects_cross_origin_absolute_target(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;

        let action_request = ActionRequest {
            parameter: "ForceRestart".to_string(),
        };

        let bmc = create_test_bmc(&mock_server);

        let action = create_test_action(
            "https://bmc.example.evil/redfish/v1/systems/1/Actions/ComputerSystem.Reset",
        );

        let error = bmc
            .action(&action, &action_request)
            .await
            .expect_err("expected cross-origin action target error");

        assert!(matches!(error, BmcError::InvalidRequest(_)));

        Ok(())
    }

    #[tokio::test]
    async fn test_action_request_empty_body_returns_empty() -> Result<(), Box<dyn std::error::Error>>
    {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/systems/1/Actions/ComputerSystem.Reset";

        let action_request = ActionRequest {
            parameter: "ForceRestart".to_string(),
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let action = create_test_action(action_path);
        let response = bmc.action(&action, &action_request).await?;

        assert!(matches!(response, ModificationResponse::Empty));

        Ok(())
    }

    #[tokio::test]
    async fn test_action_success_message_with_location_and_unit_output_returns_empty(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/systems/1/Actions/ComputerSystem.Reset";
        let task_path = "/redfish/v1/TaskService/Tasks/42";

        let action_request = ActionRequest {
            parameter: "ForceRestart".to_string(),
        };

        let success_body = serde_json::json!({
            "error": {
                "code": "Base.1.8.Success",
                "message": "Successfully Completed Request"
            }
        });

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Location", task_path)
                    .set_body_json(&success_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let action: nv_redfish_core::Action<ActionRequest, ()> =
            serde_json::from_value(serde_json::json!({ "target": action_path }))?;

        let response = bmc.action(&action, &action_request).await?;

        assert!(matches!(response, ModificationResponse::Empty));

        Ok(())
    }

    #[tokio::test]
    async fn test_create_success_message_with_location_returns_entity_reference(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mock_server = MockServer::start().await;
        let collection_path = "/redfish/v1/Managers/1/Oem/Dell/Jobs";
        let job_path = "/redfish/v1/Managers/1/Oem/Dell/Jobs/JID_42";
        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };
        let success_body = serde_json::json!({
            "@Message.ExtendedInfo": [{
                "Message": "The request completed successfully.",
                "MessageArgs": [],
                "MessageId": "Base.1.12.Success",
                "Resolution": "None",
                "Severity": "OK"
            }]
        });

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("Location", job_path)
                    .set_body_json(&success_body),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let collection_id = create_odata_id(collection_path);
        let response = bmc
            .create::<CreateRequest, LocationResource>(&collection_id, &create_request)
            .await?;

        let ModificationResponse::Entity(entity) = response else {
            return Err(String::from("expected entity reference response").into());
        };
        assert_eq!(entity.odata_id.to_string(), job_path);

        Ok(())
    }

    #[tokio::test]
    async fn test_get_request_4xx_error() {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1/nonexistent";

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.get::<TestResource>(&resource_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, BmcError::InvalidResponse { .. }));
    }

    #[tokio::test]
    async fn test_action_request_5xx_server_error() {
        let mock_server = MockServer::start().await;
        let action_path = "/redfish/v1/systems/1/Actions/ComputerSystem.Reset";

        let action_request = ActionRequest {
            parameter: "InvalidParameter".to_string(),
        };

        Mock::given(method("POST"))
            .and(path(action_path))
            .and(body_json(&action_request))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let action = create_test_action(action_path);
        let result = bmc.action(&action, &action_request).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, BmcError::InvalidResponse { .. }));
    }

    #[tokio::test]
    async fn test_custom_headers_in_get_request() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;

        let test_resource =
            create_test_resource(resource_path, Some("123"), names::TEST_SYSTEM, 42);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("X-Custom-Header", "custom-value"))
            .and(header("X-Auth-Token", "test-token-12345"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut custom_headers = http::HeaderMap::new();
        custom_headers.insert("X-Custom-Header", "custom-value".parse().unwrap());
        custom_headers.insert("X-Auth-Token", "test-token-12345".parse().unwrap());

        let bmc = create_test_bmc_with_custom_headers(&mock_server, custom_headers);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.get::<TestResource>(&resource_id).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.name, names::TEST_SYSTEM);
        assert_eq!(retrieved.value, 42);
    }

    #[tokio::test]
    async fn test_custom_headers_in_post_request() {
        let mock_server = MockServer::start().await;
        let collection_path = paths::SYSTEMS_1;

        let create_request = CreateRequest {
            name: names::TEST_SYSTEM.to_string(),
            value: 999,
        };

        let created_resource =
            create_test_resource("/redfish/v1/systems/new", None, names::TEST_SYSTEM, 999);

        Mock::given(method("POST"))
            .and(path(collection_path))
            .and(body_json(&create_request))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("X-Vendor-Specific", "vendor-value"))
            .and(header("X-Request-Id", "req-123"))
            .respond_with(ResponseTemplate::new(201).set_body_json(&created_resource))
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut custom_headers = http::HeaderMap::new();
        custom_headers.insert("X-Vendor-Specific", "vendor-value".parse().unwrap());
        custom_headers.insert("X-Request-Id", "req-123".parse().unwrap());

        let bmc = create_test_bmc_with_custom_headers(&mock_server, custom_headers);

        let collection_id = create_odata_id(collection_path);
        let result = bmc
            .create::<CreateRequest, TestResource>(&collection_id, &create_request)
            .await;

        assert!(result.is_ok());
        let created = match result.unwrap() {
            ModificationResponse::Entity(created) => created,
            _ => panic!("expected entity response"),
        };
        assert_eq!(created.name, names::TEST_SYSTEM);
        assert_eq!(created.value, 999);
    }

    #[tokio::test]
    async fn test_custom_headers_in_delete_request() {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1/systems/1";

        Mock::given(method("DELETE"))
            .and(path(resource_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("X-Delete-Reason", "decommissioned"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut custom_headers = http::HeaderMap::new();
        custom_headers.insert("X-Delete-Reason", "decommissioned".parse().unwrap());

        let bmc = create_test_bmc_with_custom_headers(&mock_server, custom_headers);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.delete::<TestResource>(&resource_id).await;

        assert!(result.is_ok());
    }
}
