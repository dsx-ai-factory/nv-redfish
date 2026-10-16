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
mod cache_integration_tests {
    use std::{error::Error, sync::Arc};

    use crate::common::test_utils::*;

    use nv_redfish_bmc_http::{
        reqwest::{BmcError, Client},
        CacheSettings, HttpBmc,
    };
    use nv_redfish_core::query::{ExpandQuery, FilterQuery};
    use nv_redfish_core::Bmc;
    use url::Url;
    use wiremock::{
        matchers::{header, method, path, query_param},
        Mock, MockServer, ResponseTemplate,
    };

    async fn mount_distinct_query_cache_mocks(
        mock_server: &MockServer,
        resource_path: &str,
        query_name: &str,
        first_query_value: &str,
        first_resource: &TestResource,
        second_query_value: &str,
        second_resource: &TestResource,
        etag_value: &str,
    ) {
        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(query_param(query_name, first_query_value))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(first_resource)
                    .insert_header("etag", etag_value),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(query_param(query_name, second_query_value))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(second_resource)
                    .insert_header("etag", etag_value),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(query_param(query_name, first_query_value))
            .and(header("if-none-match", etag_value))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(query_param(query_name, second_query_value))
            .and(header("if-none-match", etag_value))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(mock_server)
            .await;
    }

    #[tokio::test]
    async fn test_initial_request_caches_resource() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::CHASSIS_1;
        let etag_value = "abc123";

        let test_resource =
            create_test_resource(resource_path, Some(etag_value), names::TEST_CHASSIS, 100);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&test_resource)
                    .insert_header("etag", etag_value),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.get::<TestResource>(&resource_id).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.name, names::TEST_CHASSIS);
        assert_eq!(retrieved.value, 100);
        assert_eq!(retrieved.etag.as_ref().unwrap().to_string(), etag_value);
    }

    #[tokio::test]
    async fn test_304_not_modified_serves_from_cache() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::MANAGERS_1;
        let etag_value = "def345";

        let test_resource =
            create_test_resource(resource_path, Some(etag_value), names::TEST_MANAGER, 200);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&test_resource)
                    .insert_header("etag", etag_value),
            )
            .up_to_n_times(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("if-none-match", etag_value))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);

        let result1 = bmc.get::<TestResource>(&resource_id).await;
        assert!(result1.is_ok());
        let retrieved1 = result1.unwrap();
        assert_eq!(retrieved1.name, names::TEST_MANAGER);

        let result2 = bmc.get::<TestResource>(&resource_id).await;
        assert!(result2.is_ok());
        let retrieved2 = result2.unwrap();

        assert_eq!(retrieved1.name, retrieved2.name);
        assert_eq!(retrieved1.value, retrieved2.value);

        assert!(Arc::ptr_eq(&retrieved1, &retrieved2));
    }

    #[tokio::test]
    async fn test_expand_cache_key_includes_query() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;
        let etag_value = "shared-expand";

        let shallow_resource =
            create_test_resource(resource_path, Some(etag_value), "Shallow System", 1);
        let deep_resource = create_test_resource(resource_path, Some(etag_value), "Deep System", 2);

        mount_distinct_query_cache_mocks(
            &mock_server,
            resource_path,
            "$expand",
            ".($levels=1)",
            &shallow_resource,
            ".($levels=2)",
            &deep_resource,
            etag_value,
        )
        .await;

        let bmc = create_test_bmc(&mock_server);
        let resource_id = create_odata_id(resource_path);

        let shallow = bmc
            .expand::<TestResource>(&resource_id, ExpandQuery::current().levels(1))
            .await
            .unwrap();
        assert_eq!(shallow.name, "Shallow System");
        assert_eq!(shallow.value, 1);

        let deep = bmc
            .expand::<TestResource>(&resource_id, ExpandQuery::current().levels(2))
            .await
            .unwrap();
        assert_eq!(deep.name, "Deep System");
        assert_eq!(deep.value, 2);

        let shallow_cached = bmc
            .expand::<TestResource>(&resource_id, ExpandQuery::current().levels(1))
            .await
            .unwrap();
        assert_eq!(shallow_cached.name, "Shallow System");
        assert_eq!(shallow_cached.value, 1);
        assert!(Arc::ptr_eq(&shallow, &shallow_cached));

        let deep_cached = bmc
            .expand::<TestResource>(&resource_id, ExpandQuery::current().levels(2))
            .await
            .unwrap();
        assert_eq!(deep_cached.name, "Deep System");
        assert_eq!(deep_cached.value, 2);
        assert!(Arc::ptr_eq(&deep, &deep_cached));
    }

    #[tokio::test]
    async fn test_filter_cache_key_includes_query() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;
        let etag_value = "shared-filter";

        let smaller_resource =
            create_test_resource(resource_path, Some(etag_value), "Smaller System", 10);
        let larger_resource =
            create_test_resource(resource_path, Some(etag_value), "Larger System", 100);

        mount_distinct_query_cache_mocks(
            &mock_server,
            resource_path,
            "$filter",
            "value gt 10",
            &smaller_resource,
            "value gt 100",
            &larger_resource,
            etag_value,
        )
        .await;

        let bmc = create_test_bmc(&mock_server);
        let resource_id = create_odata_id(resource_path);

        let smaller = bmc
            .filter::<TestResource>(&resource_id, FilterQuery::gt(&"value", 10))
            .await
            .unwrap();
        assert_eq!(smaller.name, "Smaller System");
        assert_eq!(smaller.value, 10);

        let larger = bmc
            .filter::<TestResource>(&resource_id, FilterQuery::gt(&"value", 100))
            .await
            .unwrap();
        assert_eq!(larger.name, "Larger System");
        assert_eq!(larger.value, 100);

        let smaller_cached = bmc
            .filter::<TestResource>(&resource_id, FilterQuery::gt(&"value", 10))
            .await
            .unwrap();
        assert_eq!(smaller_cached.name, "Smaller System");
        assert_eq!(smaller_cached.value, 10);
        assert!(Arc::ptr_eq(&smaller, &smaller_cached));

        let larger_cached = bmc
            .filter::<TestResource>(&resource_id, FilterQuery::gt(&"value", 100))
            .await
            .unwrap();
        assert_eq!(larger_cached.name, "Larger System");
        assert_eq!(larger_cached.value, 100);
        assert!(Arc::ptr_eq(&larger, &larger_cached));
    }

    #[tokio::test]
    async fn test_etag_changed_updates_cache() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;
        let old_etag = "old123";
        let new_etag = "new456";

        let old_resource = create_test_resource(resource_path, Some(old_etag), "Old System", 1);

        let new_resource = create_test_resource(resource_path, Some(new_etag), "Updated System", 2);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&old_resource)
                    .insert_header("etag", old_etag),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("if-none-match", old_etag))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&new_resource)
                    .insert_header("etag", new_etag),
            )
            .up_to_n_times(1)
            .expect(1)
            .mount(&mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("if-none-match", new_etag))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);

        let result1 = bmc.get::<TestResource>(&resource_id).await;
        assert!(result1.is_ok());
        let retrieved1 = result1.unwrap();
        assert_eq!(retrieved1.name, "Old System");
        assert_eq!(retrieved1.value, 1);

        let result2 = bmc.get::<TestResource>(&resource_id).await;
        assert!(result2.is_ok());
        let retrieved2 = result2.unwrap();
        assert_eq!(retrieved2.name, "Updated System");
        assert_eq!(retrieved2.value, 2);

        assert!(!Arc::ptr_eq(&retrieved1, &retrieved2));

        let result3 = bmc.get::<TestResource>(&resource_id).await;
        assert!(result3.is_ok());
        let retrieved3 = result3.unwrap();
        assert_eq!(retrieved3.name, "Updated System");
        assert_eq!(retrieved3.value, 2);
        assert!(Arc::ptr_eq(&retrieved2, &retrieved3));
    }

    #[tokio::test]
    async fn test_cache_miss_error() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::NONEXISTENT;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.get::<TestResource>(&resource_id).await;

        assert!(result.is_err());
        let error = result.unwrap_err();
        assert!(matches!(error, BmcError::CacheMiss));
    }

    #[tokio::test]
    async fn zero_capacity_disables_etag_and_body_caching() -> Result<(), Box<dyn Error>> {
        let mock_server = MockServer::start().await;
        let resource_path = paths::SYSTEMS_1;
        let etag_value = "zero-capacity-etag";
        let test_resource =
            create_test_resource(resource_path, Some(etag_value), names::TEST_SYSTEM, 42);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&test_resource)
                    .insert_header("etag", etag_value),
            )
            .expect(2)
            .mount(&mock_server)
            .await;

        let client = Client::new()?;
        let bmc = HttpBmc::new(
            client,
            Url::parse(&mock_server.uri())?,
            create_test_credentials(),
            // Capacity zero must disable both response-body and ETag caching.
            CacheSettings::with_capacity(0),
        );

        let resource_id = create_odata_id(resource_path);

        let first_result = bmc.get::<TestResource>(&resource_id).await?;
        let second_result = bmc.get::<TestResource>(&resource_id).await?;

        mock_server.verify().await;

        let Some(received_requests) = mock_server.received_requests().await else {
            panic!("request recording should be enabled");
        };

        assert_eq!(first_result.name, names::TEST_SYSTEM);
        assert_eq!(first_result.value, 42);
        assert_eq!(second_result.name, names::TEST_SYSTEM);
        assert_eq!(second_result.value, 42);
        assert!(!Arc::ptr_eq(&first_result, &second_result));
        assert_eq!(received_requests.len(), 2);
        assert!(received_requests
            .iter()
            .all(|request| !request.headers.contains_key("if-none-match")));

        Ok(())
    }

    #[tokio::test]
    async fn test_etag_cache_from_header() {
        let mock_server = MockServer::start().await;
        let resource_path = paths::CHASSIS_1;
        let etag_value = "headeretag";

        let test_resource = create_test_resource(resource_path, None, names::TEST_CHASSIS, 100);

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&test_resource)
                    .insert_header("etag", etag_value),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);

        let resource_id = create_odata_id(resource_path);
        let result = bmc.get::<TestResource>(&resource_id).await;

        assert!(result.is_ok());
        let retrieved = result.unwrap();
        assert_eq!(retrieved.etag.as_ref().unwrap().to_string(), etag_value);
    }

    /// Sets `value`, so a result shows that the repair ran.
    struct SetValue(serde_json::Value);

    impl nv_redfish_core::Repair for SetValue {
        fn repair(&self, document: &mut serde_json::Value) {
            document["value"] = self.0.clone();
        }
    }

    #[tokio::test]
    async fn test_a_repaired_entity_is_served_from_the_cache() -> Result<(), Box<dyn Error>> {
        let mock_server = MockServer::start().await;
        let resource_path = paths::CHASSIS_1;
        let etag_value = "repaired123";
        let test_resource =
            create_test_resource(resource_path, Some(etag_value), names::TEST_CHASSIS, 0);

        // The first read fetches the body; the second revalidates it.
        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("if-none-match", etag_value))
            .respond_with(ResponseTemplate::new(304))
            .with_priority(1)
            .expect(1)
            .mount(&mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(&test_resource)
                    .insert_header("etag", etag_value),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let id = create_odata_id(resource_path);
        let repair = SetValue(10.into());

        let read = bmc.get_repaired::<TestResource>(&id, &repair).await?;
        assert_eq!(read.value, 10);
        let cached = bmc.get_repaired::<TestResource>(&id, &repair).await?;
        assert!(
            Arc::ptr_eq(&read, &cached),
            "the repaired entity is served from the cache"
        );
        Ok(())
    }

    #[tokio::test]
    async fn test_a_document_that_does_not_decode_after_repair_is_a_decode_error(
    ) -> Result<(), Box<dyn Error>> {
        let mock_server = MockServer::start().await;
        let resource_path = paths::CHASSIS_1;
        let test_resource = create_test_resource(resource_path, None, names::TEST_CHASSIS, 0);
        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_resource))
            .mount(&mock_server)
            .await;

        let bmc = create_test_bmc(&mock_server);
        let breaks = SetValue("not a number".into());
        let result = bmc
            .get_repaired::<TestResource>(&create_odata_id(resource_path), &breaks)
            .await;
        match result {
            Err(nv_redfish_core::RepairError::Decode(error)) => {
                assert_eq!(error.path().to_string(), "value");
            }
            other => panic!("expected a decode error, got {:?}", other),
        }
        Ok(())
    }
}
