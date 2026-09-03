// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

mod common;

#[cfg(all(feature = "reqwest", feature = "http-extras"))]
mod tests {
    use std::future::Future;
    use std::num::NonZeroUsize;
    use std::task::Poll;
    use std::time::Duration;

    use crate::common::test_utils::{create_test_credentials, TestResource};

    use nv_redfish_bmc_http::reqwest::BmcError;
    #[cfg(feature = "update-service-deprecated")]
    use nv_redfish_bmc_http::HttpPushUriUpdateRequest;
    use nv_redfish_bmc_http::{
        BmcCredentials, CacheSettings, ConcurrencyLimitedBmc, HttpBmc, HttpClient,
        MultipartUpdateRequest,
    };
    use nv_redfish_core::query::ExpandQuery;
    #[cfg(feature = "update-service-deprecated")]
    use nv_redfish_core::UploadStream;
    use nv_redfish_core::{
        Action, Bmc, BoxTryStream, DataStream, FilterQuery, ModificationResponse, ODataETag,
        ODataId, SessionCreateResponse, UploadReader,
    };

    use futures_util::io::Cursor;
    use http::HeaderMap;
    use serde::{de::DeserializeOwned, Deserialize, Serialize};
    use serde_json::Value as JsonValue;
    use tokio::sync::{mpsc, oneshot};
    use tokio_test::{assert_pending, assert_ready_ok};
    use url::Url;

    const SSE_URI: &str = "/redfish/v1/EventService/SSE";

    #[derive(Debug)]
    enum TestResponse {
        Resource(Option<ODataETag>),
        Retry,
        SseEstablished,
        Cached,
    }

    struct TransportAttempt {
        path: String,
        response: oneshot::Sender<TestResponse>,
    }

    #[derive(Clone)]
    struct ControlledClient {
        attempts: mpsc::UnboundedSender<TransportAttempt>,
    }

    impl ControlledClient {
        async fn response_for(&self, path: String) -> Result<TestResponse, BmcError> {
            let (response_tx, response_rx) = oneshot::channel();

            self.attempts
                .send(TransportAttempt {
                    path,
                    response: response_tx,
                })
                .map_err(|_| BmcError::InvalidRequest("test transport stopped".to_owned()))?;

            response_rx
                .await
                .map_err(|_| BmcError::InvalidRequest("test response was dropped".to_owned()))
        }

        async fn modification_response<T>(
            &self,
            path: String,
        ) -> Result<ModificationResponse<T>, BmcError> {
            match self.response_for(path).await? {
                TestResponse::Resource(_) => Ok(ModificationResponse::Empty),
                TestResponse::Retry | TestResponse::SseEstablished | TestResponse::Cached => Err(
                    BmcError::InvalidRequest("unexpected test transport response".to_owned()),
                ),
            }
        }
    }

    impl HttpClient for ControlledClient {
        type Error = BmcError;

        fn get<T>(
            &self,
            url: Url,
            _credentials: &BmcCredentials,
            _etag: Option<ODataETag>,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<T, Self::Error>> + Send
        where
            T: DeserializeOwned + Send + Sync,
        {
            let path = url.path().to_owned();
            let url = url.clone();

            async move {
                loop {
                    match self.response_for(path.clone()).await? {
                        TestResponse::Resource(etag) => {
                            let value = serde_json::json!({
                                "@odata.id": path,
                                "@odata.etag": etag,
                                "name": "resource",
                                "value": 1
                            });

                            return serde_json::from_value(value).map_err(BmcError::DecodeError);
                        }
                        TestResponse::Retry => {}
                        TestResponse::SseEstablished => {
                            return Err(BmcError::InvalidRequest(
                                "unexpected SSE response for GET".to_owned(),
                            ));
                        }
                        TestResponse::Cached => {
                            return Err(BmcError::InvalidResponse {
                                url,
                                status: http::StatusCode::NOT_MODIFIED,
                                text: String::new(),
                            });
                        }
                    }
                }
            }
        }

        fn post<B, T>(
            &self,
            url: Url,
            _body: &B,
            _credentials: &BmcCredentials,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
        where
            B: Serialize + Send + Sync,
            T: DeserializeOwned + Send + Sync,
        {
            self.modification_response(url.path().to_owned())
        }

        fn post_session<B, T>(
            &self,
            url: Url,
            _body: &B,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<SessionCreateResponse<T>, Self::Error>> + Send
        where
            B: Serialize + Send + Sync,
            T: DeserializeOwned + Send + Sync,
        {
            let path = url.path().to_owned();

            async move {
                let _response = self.modification_response::<T>(path.clone()).await?;
                let entity =
                    serde_json::from_value(JsonValue::Null).map_err(BmcError::DecodeError)?;

                Ok(SessionCreateResponse {
                    entity,
                    auth_token: "test-token".to_owned(),
                    location: ODataId::from(path),
                })
            }
        }

        fn post_multipart_update<U, V, T>(
            &self,
            url: Url,
            _request: MultipartUpdateRequest<'_, U, V>,
            _credentials: &BmcCredentials,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
        where
            U: UploadReader,
            T: DeserializeOwned + Send + Sync,
            V: Serialize + Send + Sync,
        {
            self.modification_response(url.path().to_owned())
        }

        #[cfg(feature = "update-service-deprecated")]
        fn post_http_push_uri_update<U, T>(
            &self,
            url: Url,
            _request: HttpPushUriUpdateRequest<U>,
            _credentials: &BmcCredentials,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
        where
            U: UploadReader,
            T: DeserializeOwned + Send + Sync,
        {
            self.modification_response(url.path().to_owned())
        }

        fn patch<B, T>(
            &self,
            url: Url,
            _etag: ODataETag,
            _body: &B,
            _credentials: &BmcCredentials,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
        where
            B: Serialize + Send + Sync,
            T: DeserializeOwned + Send + Sync,
        {
            self.modification_response(url.path().to_owned())
        }

        fn delete<T>(
            &self,
            url: Url,
            _credentials: &BmcCredentials,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
        where
            T: DeserializeOwned + Send + Sync,
        {
            self.modification_response(url.path().to_owned())
        }

        fn sse<T: Sized + for<'de> Deserialize<'de> + Send>(
            &self,
            url: Url,
            _credentials: &BmcCredentials,
            _custom_headers: &HeaderMap,
        ) -> impl Future<Output = Result<BoxTryStream<T, Self::Error>, Self::Error>> + Send
        {
            let path = url.path().to_owned();

            async move {
                match self.response_for(path).await? {
                    TestResponse::SseEstablished => {
                        let stream = futures_util::stream::once(async {
                            std::future::pending::<()>().await;
                            Err::<T, BmcError>(BmcError::InvalidRequest(
                                "unreachable test stream item".to_owned(),
                            ))
                        });

                        let stream: BoxTryStream<T, BmcError> = Box::pin(stream);

                        Ok(stream)
                    }
                    TestResponse::Resource(_) | TestResponse::Retry | TestResponse::Cached => Err(
                        BmcError::InvalidRequest("unexpected non-SSE response for SSE".to_owned()),
                    ),
                }
            }
        }
    }

    struct TransportController {
        attempts: mpsc::UnboundedReceiver<TransportAttempt>,
    }

    impl TransportController {
        fn next_attempt(&mut self) -> TransportAttempt {
            self.attempts
                .try_recv()
                .expect("a polled transport operation must report its attempt")
        }

        fn assert_no_attempt(&mut self) {
            assert!(
                matches!(
                    self.attempts.try_recv(),
                    Err(mpsc::error::TryRecvError::Empty)
                ),
                "an operation entered the transport while it should have been waiting"
            );
        }
    }

    fn controlled_client() -> (ControlledClient, TransportController) {
        let (attempts_tx, attempts) = mpsc::unbounded_channel();

        (
            ControlledClient {
                attempts: attempts_tx,
            },
            TransportController { attempts },
        )
    }

    type LimitedTestBmc = ConcurrencyLimitedBmc<HttpBmc<ControlledClient>>;

    fn create_bmc(
        client: ControlledClient,
        limit: NonZeroUsize,
        cache_size: usize,
    ) -> LimitedTestBmc {
        let bmc = HttpBmc::new(
            client,
            Url::parse("http://bmc.example").expect("test endpoint must be valid"),
            create_test_credentials(),
            CacheSettings::with_capacity(cache_size),
        );

        bmc.with_request_concurrency_limit(limit)
    }

    fn respond(attempt: TransportAttempt, response: TestResponse) {
        attempt
            .response
            .send(response)
            .expect("request future must accept its response");
    }

    fn respond_cached(attempt: TransportAttempt) {
        attempt
            .response
            .send(TestResponse::Cached)
            .expect("request future must accept its response");
    }

    #[derive(Clone, Copy)]
    enum Operation {
        Get,
        Expand,
        Filter,
        Create,
        CreateSession,
        Update,
        Delete,
        Action,
        MultipartUpdate,
        #[cfg(feature = "update-service-deprecated")]
        HttpPushUriUpdate,
    }

    impl Operation {
        const fn path(self) -> &'static str {
            match self {
                Self::Get => "/get",
                Self::Expand => "/expand",
                Self::Filter => "/filter",
                Self::Create => "/create",
                Self::CreateSession => "/session",
                Self::Update => "/update",
                Self::Delete => "/delete",
                Self::Action => "/action",
                Self::MultipartUpdate => "/multipart-update",
                #[cfg(feature = "update-service-deprecated")]
                Self::HttpPushUriUpdate => "/http-push-uri-update",
            }
        }
    }

    async fn run_operation(bmc: &LimitedTestBmc, operation: Operation) -> Result<(), BmcError> {
        let path = operation.path();
        let id = ODataId::from(path.to_owned());

        match operation {
            Operation::Get => {
                let _response = bmc.get::<TestResource>(&id).await?;
            }
            Operation::Expand => {
                let _response = bmc
                    .expand::<TestResource>(&id, ExpandQuery::current())
                    .await?;
            }
            Operation::Filter => {
                let _response = bmc
                    .filter::<TestResource>(&id, FilterQuery::eq(&"value", 1))
                    .await?;
            }
            Operation::Create => {
                let _response = bmc
                    .create::<JsonValue, JsonValue>(&id, &serde_json::json!({}))
                    .await?;
            }
            Operation::CreateSession => {
                bmc.create_session::<JsonValue, JsonValue>(&id, &serde_json::json!({}))
                    .await?;
            }
            Operation::Update => {
                let _response = bmc
                    .update::<JsonValue, JsonValue>(&id, None, &serde_json::json!({}))
                    .await?;
            }
            Operation::Delete => {
                let _response = bmc.delete::<TestResource>(&id).await?;
            }
            Operation::Action => {
                let action: Action<JsonValue, JsonValue> =
                    serde_json::from_value(serde_json::json!({ "target": path }))
                        .map_err(BmcError::DecodeError)?;

                let _response = bmc.action(&action, &serde_json::json!({})).await?;
            }
            Operation::MultipartUpdate => {
                let update_parameters = serde_json::json!({});

                let request = MultipartUpdateRequest {
                    update_parameters: &update_parameters,
                    update_stream: DataStream::new("firmware.bin", Cursor::new(Vec::<u8>::new())),
                    oem_parts: Vec::new(),
                    upload_timeout: Duration::from_secs(60),
                };

                let _response = bmc
                    .multipart_update::<_, _, JsonValue>(path, request)
                    .await?;
            }
            #[cfg(feature = "update-service-deprecated")]
            Operation::HttpPushUriUpdate => {
                let request = HttpPushUriUpdateRequest {
                    update_stream: UploadStream::new(Cursor::new(Vec::<u8>::new())),
                    upload_timeout: Duration::from_secs(60),
                };

                let _response = bmc
                    .http_push_uri_update::<_, JsonValue>(path, request)
                    .await?;
            }
        }

        Ok(())
    }

    fn assert_operation_waits_for_permit(operation: Operation) {
        let (client, mut transport) = controlled_client();
        let bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let holder_id = ODataId::from("/holder".to_owned());

        let mut holder = tokio_test::task::spawn(bmc.get::<TestResource>(&holder_id));
        assert_pending!(holder.poll());
        let holder_attempt = transport.next_attempt();

        let mut operation_task = tokio_test::task::spawn(run_operation(&bmc, operation));
        assert_pending!(operation_task.poll());
        transport.assert_no_attempt();

        respond(holder_attempt, TestResponse::Resource(None));
        assert_ready_ok!(holder.poll());
        assert!(operation_task.is_woken());
        assert_pending!(operation_task.poll());
        let operation_attempt = transport.next_attempt();

        assert_eq!(operation_attempt.path, operation.path());
        respond(operation_attempt, TestResponse::Resource(None));
        assert_ready_ok!(operation_task.poll());
    }

    #[test]
    fn test_missed_cache_error_on_concurrent_requests() -> Result<(), Box<dyn std::error::Error>> {
        let (client, mut transport) = controlled_client();
        let limit = NonZeroUsize::new(2).expect("test limit must be non-zero");
        let bmc = create_bmc(client, limit, 1);

        let first_id = ODataId::from("/first".to_owned());
        let second_id = ODataId::from("/second".to_owned());

        let mut first = tokio_test::task::spawn(bmc.get::<TestResource>(&first_id));
        assert_pending!(first.poll());
        //Caching the first response
        let first_attempt = transport.next_attempt();
        respond(
            first_attempt,
            TestResponse::Resource(Some("test".to_string().into())),
        );
        assert_ready_ok!(first.poll());

        //Request it again, client should see it's cached and expects:
        //StatusCode::NOT_MODIFIED -> Get from cache
        let mut first_pretend_cached = tokio_test::task::spawn(bmc.get::<TestResource>(&first_id));
        assert_pending!(first_pretend_cached.poll());
        let first_attempt_pretend_cached = transport.next_attempt();

        //Send second request to overwrite the cache meanwhile
        let mut second = tokio_test::task::spawn(bmc.get::<TestResource>(&second_id));
        assert_pending!(second.poll());
        let second_attempt = transport.next_attempt();
        respond(
            second_attempt,
            TestResponse::Resource(Some("another_tag".to_string().into())),
        );
        assert_ready_ok!(second.poll());

        //Confirm StatusCode::NOT_MODIFIED for the first
        respond_cached(first_attempt_pretend_cached);
        assert_ready_ok!(first_pretend_cached.poll());

        Ok(())
    }

    #[test]
    fn a_released_permit_goes_to_the_longest_waiter_not_a_barger() {
        let (client, mut transport) = controlled_client();
        let bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let holder_id = ODataId::from("/holder".to_owned());
        let waiter_id = ODataId::from("/waiter".to_owned());
        let barger_id = ODataId::from("/barger".to_owned());

        // The holder takes the only permit; then two operations queue in
        // arrival order.
        let mut holder = tokio_test::task::spawn(bmc.get::<TestResource>(&holder_id));
        assert_pending!(holder.poll());
        let holder_attempt = transport.next_attempt();

        let mut waiter = tokio_test::task::spawn(bmc.get::<TestResource>(&waiter_id));
        assert_pending!(waiter.poll());
        let mut barger = tokio_test::task::spawn(bmc.get::<TestResource>(&barger_id));
        assert_pending!(barger.poll());
        transport.assert_no_attempt();

        // Release the permit, then poll the barger FIRST: arrival order
        // must beat poll order, or a hot caller starves the queue.
        respond(holder_attempt, TestResponse::Resource(None));
        assert_ready_ok!(holder.poll());
        assert_pending!(barger.poll());
        transport.assert_no_attempt();

        assert!(waiter.is_woken());
        assert_pending!(waiter.poll());
        let waiter_attempt = transport.next_attempt();
        assert_eq!(waiter_attempt.path, "/waiter");
        respond(waiter_attempt, TestResponse::Resource(None));
        assert_ready_ok!(waiter.poll());

        assert!(barger.is_woken());
        assert_pending!(barger.poll());
        let barger_attempt = transport.next_attempt();
        assert_eq!(barger_attempt.path, "/barger");
        respond(barger_attempt, TestResponse::Resource(None));
        assert_ready_ok!(barger.poll());
    }

    #[test]
    fn limit_two_allows_two_operations_and_blocks_a_third() {
        let (client, mut transport) = controlled_client();
        let limit = NonZeroUsize::new(2).expect("test limit must be non-zero");
        let bmc = create_bmc(client, limit, 0);
        let first_id = ODataId::from("/first".to_owned());
        let second_id = ODataId::from("/second".to_owned());
        let third_id = ODataId::from("/third".to_owned());

        let mut first = tokio_test::task::spawn(bmc.get::<TestResource>(&first_id));
        let mut second = tokio_test::task::spawn(bmc.get::<TestResource>(&second_id));
        assert_pending!(first.poll());
        assert_pending!(second.poll());
        let first_attempt = transport.next_attempt();
        let second_attempt = transport.next_attempt();

        let mut third = tokio_test::task::spawn(bmc.get::<TestResource>(&third_id));
        assert_pending!(third.poll());
        transport.assert_no_attempt();

        respond(first_attempt, TestResponse::Resource(None));
        assert_ready_ok!(first.poll());

        assert!(third.is_woken());
        assert_pending!(third.poll());
        let third_attempt = transport.next_attempt();

        assert_eq!(third_attempt.path, "/third");
        respond(second_attempt, TestResponse::Resource(None));
        respond(third_attempt, TestResponse::Resource(None));
        assert_ready_ok!(second.poll());
        assert_ready_ok!(third.poll());
    }

    #[test]
    fn shared_http_client_has_independent_per_bmc_limits() {
        let (client, mut transport) = controlled_client();
        let first_bmc = create_bmc(client.clone(), NonZeroUsize::MIN, 0);
        let second_bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let first_id = ODataId::from("/first".to_owned());
        let second_id = ODataId::from("/second".to_owned());

        let mut first = tokio_test::task::spawn(first_bmc.get::<TestResource>(&first_id));
        assert_pending!(first.poll());
        let first_attempt = transport.next_attempt();

        let mut second = tokio_test::task::spawn(second_bmc.get::<TestResource>(&second_id));
        assert_pending!(second.poll());
        let second_attempt = transport.next_attempt();

        assert_eq!(second_attempt.path, "/second");
        respond(first_attempt, TestResponse::Resource(None));
        respond(second_attempt, TestResponse::Resource(None));
        assert_ready_ok!(first.poll());
        assert_ready_ok!(second.poll());
    }

    #[test]
    fn all_request_paths_wait_for_a_permit_at_limit_one() {
        for operation in [
            Operation::Get,
            Operation::Expand,
            Operation::Filter,
            Operation::Create,
            Operation::CreateSession,
            Operation::Update,
            Operation::Delete,
            Operation::Action,
            Operation::MultipartUpdate,
        ] {
            assert_operation_waits_for_permit(operation);
        }

        #[cfg(feature = "update-service-deprecated")]
        assert_operation_waits_for_permit(Operation::HttpPushUriUpdate);
    }

    #[test]
    fn retry_holds_permit_for_entire_logical_operation() {
        let (client, mut transport) = controlled_client();
        let bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let retry_id = ODataId::from("/retry".to_owned());
        let other_id = ODataId::from("/other".to_owned());

        let mut retrying = tokio_test::task::spawn(bmc.get::<TestResource>(&retry_id));
        assert_pending!(retrying.poll());
        let first_attempt = transport.next_attempt();

        let mut other = tokio_test::task::spawn(bmc.get::<TestResource>(&other_id));
        assert_pending!(other.poll());
        transport.assert_no_attempt();

        respond(first_attempt, TestResponse::Retry);
        assert_pending!(retrying.poll());
        let second_attempt = transport.next_attempt();

        assert_eq!(second_attempt.path, "/retry");
        transport.assert_no_attempt();

        respond(second_attempt, TestResponse::Resource(None));
        assert_ready_ok!(retrying.poll());

        assert!(other.is_woken());
        assert_pending!(other.poll());
        let other_attempt = transport.next_attempt();

        assert_eq!(other_attempt.path, "/other");
        respond(other_attempt, TestResponse::Resource(None));
        assert_ready_ok!(other.poll());
    }

    #[test]
    fn canceling_waiter_does_not_consume_capacity() {
        let (client, mut transport) = controlled_client();
        let bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let first_id = ODataId::from("/first".to_owned());
        let canceled_id = ODataId::from("/canceled".to_owned());
        let next_id = ODataId::from("/next".to_owned());

        let mut first = tokio_test::task::spawn(bmc.get::<TestResource>(&first_id));
        assert_pending!(first.poll());
        let first_attempt = transport.next_attempt();

        let mut canceled = tokio_test::task::spawn(bmc.get::<TestResource>(&canceled_id));
        assert_pending!(canceled.poll());
        transport.assert_no_attempt();
        drop(canceled);

        let mut next = tokio_test::task::spawn(bmc.get::<TestResource>(&next_id));
        assert_pending!(next.poll());
        transport.assert_no_attempt();
        respond(first_attempt, TestResponse::Resource(None));
        assert_ready_ok!(first.poll());

        assert!(next.is_woken());
        assert_pending!(next.poll());
        let next_attempt = transport.next_attempt();

        assert_eq!(next_attempt.path, "/next");
        respond(next_attempt, TestResponse::Resource(None));
        assert_ready_ok!(next.poll());
    }

    #[test]
    fn canceling_in_flight_operation_releases_capacity() {
        let (client, mut transport) = controlled_client();
        let bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let first_id = ODataId::from("/first".to_owned());
        let next_id = ODataId::from("/next".to_owned());

        let mut first = tokio_test::task::spawn(bmc.get::<TestResource>(&first_id));
        assert_pending!(first.poll());
        let first_attempt = transport.next_attempt();

        let mut next = tokio_test::task::spawn(bmc.get::<TestResource>(&next_id));
        assert_pending!(next.poll());
        transport.assert_no_attempt();

        drop(first);
        drop(first_attempt);

        assert!(next.is_woken());
        assert_pending!(next.poll());
        let next_attempt = transport.next_attempt();

        assert_eq!(next_attempt.path, "/next");
        respond(next_attempt, TestResponse::Resource(None));
        assert_ready_ok!(next.poll());
    }

    #[test]
    fn sse_holds_permit_during_establishment_and_releases_it_afterward() {
        let (client, mut transport) = controlled_client();
        let bmc = create_bmc(client, NonZeroUsize::MIN, 0);
        let next_id = ODataId::from("/next".to_owned());

        let mut sse = tokio_test::task::spawn(bmc.stream::<JsonValue>(SSE_URI));

        assert!(matches!(sse.poll(), Poll::Pending));
        let sse_attempt = transport.next_attempt();

        assert_eq!(sse_attempt.path, SSE_URI);

        let mut next = tokio_test::task::spawn(bmc.get::<TestResource>(&next_id));
        assert_pending!(next.poll());
        transport.assert_no_attempt();

        respond(sse_attempt, TestResponse::SseEstablished);

        let _stream = assert_ready_ok!(sse.poll());

        assert!(next.is_woken());
        assert_pending!(next.poll());
        let next_attempt = transport.next_attempt();

        assert_eq!(next_attempt.path, "/next");
        respond(next_attempt, TestResponse::Resource(None));
        assert_ready_ok!(next.poll());
    }

    #[test]
    fn omitted_limit_preserves_unlimited_transport_entry() {
        let (client, mut transport) = controlled_client();

        let bmc = HttpBmc::new(
            client,
            Url::parse("http://bmc.example").expect("test endpoint must be valid"),
            create_test_credentials(),
            CacheSettings::with_capacity(0),
        );

        let first_id = ODataId::from("/first".to_owned());
        let second_id = ODataId::from("/second".to_owned());

        let mut first = tokio_test::task::spawn(bmc.get::<TestResource>(&first_id));
        let mut second = tokio_test::task::spawn(bmc.get::<TestResource>(&second_id));
        assert_pending!(first.poll());
        assert_pending!(second.poll());
        let first_attempt = transport.next_attempt();
        let second_attempt = transport.next_attempt();

        respond(first_attempt, TestResponse::Resource(None));
        respond(second_attempt, TestResponse::Resource(None));
        assert_ready_ok!(first.poll());
        assert_ready_ok!(second.poll());
    }
}
