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

//! Implementation of [`HttpClient`] trait using reqwest crate.

use std::error::Error as StdErr;
use std::fmt;
use std::future::ready;
use std::sync::Arc;
use std::time::Duration;

use crate::schema::redfish::message::Message;
use crate::schema::redfish::redfish_error::RedfishError;
use crate::BmcCredentials;
use crate::CacheableError;
use crate::HttpClient;
#[cfg(feature = "update-service-deprecated")]
use crate::HttpPushUriUpdateRequest;
use crate::MultipartUpdateRequest;
use crate::RejectedUriReferenceError;
use crate::RequestError;

use bytes::Bytes;
use futures_util::stream::unfold;
use futures_util::Stream;
use futures_util::StreamExt as _;
use http::header;
use http::HeaderMap;
use nv_redfish_core::ActionError;
use nv_redfish_core::AsyncTask;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::DataStream;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::OemMultipartPart;
use nv_redfish_core::SessionCreateResponse;
use nv_redfish_core::UploadReader;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::UploadStream;
use reqwest::multipart::Form;
use reqwest::multipart::Part;
use reqwest::redirect::Policy as RedirectPolicy;
use reqwest::Client as ReqwestClient;
use reqwest::Error as ReqwestError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::time::sleep;
use tokio::time::timeout;
use tokio_util::compat::FuturesAsyncReadCompatExt as _;
use tokio_util::io::ReaderStream;
use url::Url;

/// Errors of reqwest implementation of the HTTP trait.
#[derive(Debug)]
pub enum BmcError {
    /// Direct mapping of underlying reqwest error.
    ReqwestError(reqwest::Error),
    /// JSON to model deserialize error with path tracking.
    JsonError(serde_path_to_error::Error<serde_json::Error>),
    /// Unexpected HTTP response.
    InvalidResponse {
        /// URL in request that caused error.
        url: url::Url,
        /// Returned status.
        status: reqwest::StatusCode,
        /// Text in the response.
        text: String,
    },
    /// SSE stream error.
    SseStreamError(sse_stream::Error),
    /// No resource found in cache.
    CacheMiss,
    /// HTTP cache error.
    CacheError(String),
    /// JSON deserialization error.
    DecodeError(serde_json::Error),
    /// JSON serialization error.
    EncodeError(serde_json::Error),
    /// Request rejected before transport.
    InvalidRequest(String),
    /// A single SSE event exceeded the configured maximum buffered size.
    SseEventTooLarge {
        /// Configured byte limit that was exceeded.
        limit: usize,
    },
    /// No SSE event was received within the configured idle timeout.
    SseIdleTimeout {
        /// Idle duration that elapsed with no event.
        idle: Duration,
    },
}

impl From<reqwest::Error> for BmcError {
    fn from(value: reqwest::Error) -> Self {
        Self::ReqwestError(value)
    }
}

impl CacheableError for BmcError {
    fn is_cached(&self) -> bool {
        match self {
            Self::InvalidResponse { status, .. } => status == &reqwest::StatusCode::NOT_MODIFIED,
            _ => false,
        }
    }

    fn cache_miss() -> Self {
        Self::CacheMiss
    }

    fn cache_error(reason: String) -> Self {
        Self::CacheError(reason)
    }
}

impl RequestError for BmcError {
    fn rejected_uri_reference(error: RejectedUriReferenceError) -> Self {
        Self::InvalidRequest(error.reason)
    }
}

impl fmt::Display for BmcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReqwestError(e) => write!(f, "HTTP client error: {e:?}"),
            Self::InvalidResponse { url, status, text } => {
                write!(
                    f,
                    "Invalid HTTP response - url: {url} status: {status} text: {text}"
                )
            }
            Self::CacheMiss => write!(f, "Resource not found in cache"),
            Self::CacheError(r) => write!(f, "Error occurred in cache {r:?}"),
            Self::JsonError(e) => write!(
                f,
                "JSON deserialization error at line {} column {} path {}: {e}",
                e.inner().line(),
                e.inner().column(),
                e.path(),
            ),
            Self::SseStreamError(e) => write!(f, "SSE stream decode error: {e}"),
            Self::DecodeError(e) => write!(f, "JSON Decode error: {e}"),
            Self::EncodeError(e) => write!(f, "JSON Encode error: {e}"),
            Self::InvalidRequest(e) => write!(f, "Invalid request: {e}"),
            Self::SseEventTooLarge { limit } => write!(
                f,
                "SSE event exceeded maximum buffered size of {limit} bytes"
            ),
            Self::SseIdleTimeout { idle } => {
                write!(f, "SSE stream idle for longer than {idle:?}")
            }
        }
    }
}

impl StdErr for BmcError {
    fn source(&self) -> Option<&(dyn StdErr + 'static)> {
        match self {
            Self::ReqwestError(e) => Some(e),
            Self::JsonError(e) => Some(e.inner()),
            Self::SseStreamError(e) => Some(e),
            Self::DecodeError(e) | Self::EncodeError(e) => Some(e),
            _ => None,
        }
    }
}

impl ActionError for BmcError {
    fn not_supported() -> Self {
        Self::InvalidRequest("action is not supported".to_string())
    }

    fn is_not_found(&self) -> bool {
        matches!(
            self,
            Self::InvalidResponse { status, .. } if *status == reqwest::StatusCode::NOT_FOUND
        )
    }
}

impl BmcError {
    /// Returns `true` if this error should terminate an SSE stream.
    ///
    /// A JSON decode error is scoped to a single event and is therefore
    /// non-fatal; every other error ends the stream.
    const fn is_stream_fatal(&self) -> bool {
        !matches!(self, Self::JsonError(_))
    }
}

/// Error produced by the byte-counting adapter that feeds the SSE decoder.
///
/// It is passed through the decoder's error channel so a transport failure and
/// the "event too large" guard can be distinguished once the decoder surfaces
/// them (see [`map_sse_error`]).
#[derive(Debug)]
enum SseByteError {
    /// Underlying transport error from the response body stream.
    Upstream(reqwest::Error),
    /// The per-event byte budget was exceeded before the event terminated.
    EventTooLarge {
        /// Configured byte limit that was exceeded.
        limit: usize,
    },
}

impl fmt::Display for SseByteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Upstream(e) => write!(f, "{e}"),
            Self::EventTooLarge { limit } => {
                write!(f, "SSE event exceeded {limit} bytes without terminating")
            }
        }
    }
}

impl StdErr for SseByteError {}

/// Translate an [`sse_stream::Error`] back into a [`BmcError`], recovering the
/// [`SseByteError`] that the byte-counting adapter passed through the decoder.
fn map_sse_error(err: sse_stream::Error) -> BmcError {
    match err {
        sse_stream::Error::Body(boxed) => match boxed.downcast::<SseByteError>() {
            Ok(inner) => match *inner {
                SseByteError::Upstream(e) => BmcError::ReqwestError(e),
                SseByteError::EventTooLarge { limit } => BmcError::SseEventTooLarge { limit },
            },
            Err(other) => BmcError::SseStreamError(sse_stream::Error::Body(other)),
        },
        other => BmcError::SseStreamError(other),
    }
}

/// Line-terminator state used by [`cap_event_bytes`] to detect SSE record
/// boundaries (blank lines) directly in the raw byte stream.
#[derive(Clone, Copy)]
enum DelimiterState {
    /// At the start of a line; a line terminator delimits the record.
    LineStart,

    /// Inside a line.
    Data,

    /// Saw `\r` after data; a following `\n` belongs to the same terminator.
    Cr,

    /// Saw `\r` at line start; the record boundary awaits an optional `\n`.
    BoundaryCr,
}

/// Bound the bytes buffered for a single, not-yet-terminated SSE event.
///
/// Counts bytes on the raw transport stream and resets the count at each SSE
/// record boundary (a blank line), aborting with [`SseByteError::EventTooLarge`]
/// once an unterminated event exceeds `limit`. Working on raw bytes (rather than
/// decoded events) means comment/heartbeat records that end in a blank line also
/// reset the budget, and a well-behaved long-lived stream is never penalized.
fn cap_event_bytes(
    bytes: impl Stream<Item = Result<Bytes, reqwest::Error>>,
    limit: usize,
) -> impl Stream<Item = Result<Bytes, SseByteError>> {
    use DelimiterState::{BoundaryCr, Cr, Data, LineStart};
    bytes.scan((0_usize, LineStart), move |(count, state), chunk| {
        let item = chunk.map_err(SseByteError::Upstream).and_then(|bytes| {
            for &b in bytes.as_ref() {
                if matches!(*state, BoundaryCr) && b != b'\n' {
                    *count = 0;
                    *state = LineStart;
                }

                // Enforce the budget before a record delimiter can reset it.
                if *count >= limit {
                    return Err(SseByteError::EventTooLarge { limit });
                }

                *count += 1;

                *state = match (*state, b) {
                    (LineStart | Cr, b'\r') => BoundaryCr,
                    (LineStart | BoundaryCr, b'\n') => {
                        *count = 0;
                        LineStart
                    }
                    (Data, b'\r') => Cr,
                    (Data | Cr, b'\n') => LineStart,
                    _ => Data,
                };
            }

            Ok(bytes)
        });
        ready(Some(item))
    })
}

/// Decode one SSE record into a typed item, or `None` for records without data
/// (e.g. comments) so they are filtered out of the stream.
fn event_to_item<T: DeserializeOwned>(
    event: Result<sse_stream::Sse, sse_stream::Error>,
) -> Option<Result<T, BmcError>> {
    match event {
        Err(err) => Some(Err(map_sse_error(err))),
        Ok(sse) => sse.data.map(|data| {
            serde_path_to_error::deserialize(&mut serde_json::Deserializer::from_str(&data))
                .map_err(BmcError::JsonError)
        }),
    }
}

/// Classifier deciding whether a response should be retried.
///
/// Receives the request and the response, returns `true` if the request
/// should be retried.
type RetryClassifier =
    dyn Fn(&reqwest::Request, &reqwest::Response) -> bool + Send + Sync + 'static;

/// Retry policy with a configurable delay between attempts.
///
/// While retries remain, the classifier is called for every received HTTP
/// response, regardless of the request method, and decides whether to retry.
/// Transport and connection errors are returned immediately. Requests with
/// non-clonable (streaming) bodies, such as multipart uploads, are sent exactly
/// once and never retried.
///
/// # Examples
///
/// Retry only `GET` requests that return `503 Service Unavailable`. The
/// classifier returns `false` for every other method, so requests such as
/// `POST`, `PATCH`, or `DELETE` are never retried:
///
/// ```rust
/// use nv_redfish_bmc_http::reqwest::{ClientParams, RetryPolicy};
/// use std::time::Duration;
///
/// let policy = RetryPolicy::new(|request, response| {
///     *request.method() == http::Method::GET
///         && response.status() == http::StatusCode::SERVICE_UNAVAILABLE
/// })
/// .max_retries(3)
/// .delay(Duration::from_millis(500));
///
/// let params = ClientParams::new().retry(policy);
/// ```
#[derive(Clone)]
pub struct RetryPolicy {
    /// Number of extra attempts after the first one.
    max_retries: u32,
    /// Fixed sleep between attempts; `None` retries immediately.
    delay: Option<Duration>,
    /// Decides whether a response should be retried.
    classifier: Arc<RetryClassifier>,
}

impl RetryPolicy {
    /// Creates a policy that retries responses accepted by `classifier`.
    ///
    /// By default no retries are performed; configure them with
    /// [`Self::max_retries`] and [`Self::delay`].
    #[must_use]
    pub fn new<F>(classifier: F) -> Self
    where
        F: Fn(&reqwest::Request, &reqwest::Response) -> bool + Send + Sync + 'static,
    {
        Self {
            max_retries: 0,
            delay: None,
            classifier: Arc::new(classifier),
        }
    }

    /// Maximum number of extra attempts after the initial request.
    #[must_use]
    pub const fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Fixed delay to sleep between attempts.
    #[must_use]
    pub const fn delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }
}

impl fmt::Debug for RetryPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RetryPolicy")
            .field("max_retries", &self.max_retries)
            .field("delay", &self.delay)
            .field("classifier", &"<closure>")
            .finish()
    }
}

/// Configuration parameters for the reqwest HTTP client.
///
/// This struct allows customizing various aspects of the reqwest client behavior,
/// including timeouts, TLS settings, and connection pooling.
///
/// # Examples
///
/// ```rust
/// use nv_redfish_bmc_http::reqwest::ClientParams;
/// use std::time::Duration;
///
/// let params = ClientParams::new()
///     .timeout(Duration::from_secs(30))
///     .connect_timeout(Duration::from_secs(10))
///     .user_agent("MyApp/1.0")
///     .accept_invalid_certs(true);
/// ```
#[derive(Debug, Clone)]
pub struct ClientParams {
    /// HTTP request timeout
    pub timeout: Option<Duration>,
    /// TCP connection timeout
    pub connect_timeout: Option<Duration>,
    /// User-Agent header value
    pub user_agent: Option<String>,
    /// Whether to accept invalid TLS certificates
    pub accept_invalid_certs: bool,

    /// Maximum number of same-origin HTTP redirects to follow.
    ///
    /// `None` uses reqwest's default redirect limit. Cross-origin redirects are always rejected.
    pub max_redirects: Option<usize>,

    /// TCP keep-alive timeout
    pub tcp_keepalive: Option<Duration>,
    /// Connection pool idle timeout
    pub pool_idle_timeout: Option<Duration>,
    /// Maximum idle connections per host
    pub pool_max_idle_per_host: Option<usize>,
    /// List of default headers, added to every request
    pub default_headers: Option<HeaderMap>,
    /// Forces use of rust TLS, enabled by default
    pub use_rust_tls: bool,
    /// Retry policy for received responses, `None` disables retries
    pub retry: Option<RetryPolicy>,
    /// SSE-specific limits applied by [`Client::sse`].
    pub sse: SseOptions,
}

/// Limits applied to Server-Sent Event streams opened by [`Client::sse`].
#[derive(Clone, Copy, Debug)]
pub struct SseOptions {
    /// Maximum bytes buffered for a single, not-yet-terminated SSE event before
    /// [`Client::sse`] aborts with [`BmcError::SseEventTooLarge`]. Guards against
    /// a server that never sends the SSE event terminator.
    pub max_event_bytes: usize,
    /// Maximum idle time between SSE events before [`Client::sse`] aborts with
    /// [`BmcError::SseIdleTimeout`]. `None` disables the check.
    pub idle_timeout: Option<Duration>,
}

impl Default for SseOptions {
    fn default() -> Self {
        Self {
            max_event_bytes: 1024 * 1024,
            idle_timeout: None,
        }
    }
}

impl Default for ClientParams {
    fn default() -> Self {
        Self {
            timeout: Some(Duration::from_secs(120)),
            connect_timeout: Some(Duration::from_secs(5)),
            user_agent: Some("nv-redfish/v1".to_string()),
            accept_invalid_certs: false,
            max_redirects: Some(10),
            tcp_keepalive: Some(Duration::from_secs(60)),
            pool_idle_timeout: Some(Duration::from_secs(90)),
            pool_max_idle_per_host: Some(1),
            default_headers: None,
            use_rust_tls: true,
            retry: None,
            sse: SseOptions::default(),
        }
    }
}

impl ClientParams {
    /// Creates new client parameters.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// See: [`reqwest::ClientBuilder::timeout`].
    #[must_use]
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// See: [`reqwest::ClientBuilder::connect_timeout`].
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// See: [`reqwest::ClientBuilder::user_agent`].
    #[must_use]
    pub fn user_agent<S: Into<String>>(mut self, user_agent: S) -> Self {
        self.user_agent = Some(user_agent.into());
        self
    }

    /// See: [`reqwest::ClientBuilder::danger_accept_invalid_certs`].
    #[must_use]
    pub const fn accept_invalid_certs(mut self, accept: bool) -> Self {
        self.accept_invalid_certs = accept;
        self
    }

    /// Sets the maximum number of same-origin redirects to follow.
    #[must_use]
    pub const fn max_redirects(mut self, max: usize) -> Self {
        self.max_redirects = Some(max);
        self
    }

    /// See: [`reqwest::ClientBuilder::tcp_keepalive`].
    #[must_use]
    pub const fn tcp_keepalive(mut self, keepalive: Duration) -> Self {
        self.tcp_keepalive = Some(keepalive);
        self
    }

    /// See: [`reqwest::ClientBuilder::pool_max_idle_per_host`].
    #[must_use]
    pub const fn pool_max_idle_per_host(mut self, pool_max_idle_per_host: usize) -> Self {
        self.pool_max_idle_per_host = Some(pool_max_idle_per_host);
        self
    }

    /// See: [`reqwest::ClientBuilder::pool_idle_timeout`].
    #[must_use]
    pub const fn idle_timeout(mut self, pool_idle_timeout: Duration) -> Self {
        self.pool_idle_timeout = Some(pool_idle_timeout);
        self
    }

    /// Clears timeout for this client.
    #[must_use]
    pub const fn no_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    /// See: [`reqwest::ClientBuilder::default_headers`].
    #[must_use]
    pub fn default_headers(mut self, default_headers: HeaderMap) -> Self {
        self.default_headers = Some(default_headers);
        self
    }

    /// Sets the [`RetryPolicy`] applied to every request.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Sets the maximum buffered size of a single, not-yet-terminated SSE event.
    ///
    /// See [`SseOptions::max_event_bytes`].
    #[must_use]
    pub const fn sse_max_event_bytes(mut self, bytes: usize) -> Self {
        self.sse.max_event_bytes = bytes;
        self
    }

    /// Sets the maximum idle time between SSE events.
    ///
    /// See [`SseOptions::idle_timeout`].
    #[must_use]
    pub const fn sse_idle_timeout(mut self, timeout: Duration) -> Self {
        self.sse.idle_timeout = Some(timeout);
        self
    }
}

/// HTTP client implementation using the reqwest library.
///
/// This provides a concrete implementation of [`HttpClient`] using the popular
/// reqwest HTTP client library. It supports all standard HTTP features including
/// TLS, authentication, and connection pooling.
#[derive(Clone)]
pub struct Client {
    inner: ReqwestClient,
    retry: Option<RetryPolicy>,
    sse: SseOptions,
}

impl Client {
    /// Create client with default [`ClientParams`].
    ///
    /// # Errors
    ///
    /// Internally it builds [`reqwest::ClientBuilder::build`]. This function
    /// transparently passes errors of this call to caller.
    pub fn new() -> Result<Self, ReqwestError> {
        Self::with_params(ClientParams::default())
    }

    /// Build client from parameters.
    ///
    /// # Errors
    ///
    /// Internally it builds [`reqwest::ClientBuilder::build`]. This function
    /// transparently passes errors of this call to caller.
    pub fn with_params(params: ClientParams) -> Result<Self, reqwest::Error> {
        let mut builder = ReqwestClient::builder();

        if params.use_rust_tls {
            builder = builder.use_rustls_tls();
        }

        if let Some(timeout) = params.timeout {
            builder = builder.timeout(timeout);
        }

        if let Some(connect_timeout) = params.connect_timeout {
            builder = builder.connect_timeout(connect_timeout);
        }

        if let Some(user_agent) = params.user_agent {
            builder = builder.user_agent(user_agent);
        }

        if params.accept_invalid_certs {
            builder = builder.danger_accept_invalid_certs(true);
        }

        // Reqwest's standard policies enforce redirect limits but still follow cross-origin
        // targets, where Redfish-specific and custom authentication headers can be forwarded.
        // Wrap the selected standard policy so its limit and error behavior remain unchanged
        // while every redirect also receives the same-origin check.
        let redirect_policy = params
            .max_redirects
            .map_or_else(RedirectPolicy::default, RedirectPolicy::limited);

        builder = builder.redirect(same_origin_redirect_policy(redirect_policy));

        if let Some(keepalive) = params.tcp_keepalive {
            builder = builder.tcp_keepalive(keepalive);
        }

        if let Some(idle_timeout) = params.pool_idle_timeout {
            builder = builder.pool_idle_timeout(idle_timeout);
        }

        if let Some(max_idle) = params.pool_max_idle_per_host {
            builder = builder.pool_max_idle_per_host(max_idle);
        }

        if let Some(default_headers) = params.default_headers {
            builder = builder.default_headers(default_headers);
        }

        Ok(Self {
            inner: builder.build()?,
            retry: params.retry,
            sse: params.sse,
        })
    }

    /// Uses a pre-built [`reqwest::Client`] as the internal client.
    ///
    /// Unlike [`Self::new`] and [`Self::with_params`], this constructor cannot install or inspect
    /// the client's redirect policy.
    ///
    /// # Security
    ///
    /// The supplied client must reject cross-origin redirects. Reqwest's default redirect policy
    /// can forward Redfish `X-Auth-Token` and arbitrary custom headers to another origin.
    #[must_use]
    pub fn with_client(client: ReqwestClient) -> Self {
        Self {
            inner: client,
            retry: None,
            sse: SseOptions::default(),
        }
    }
}

impl Client {
    /// Sends the request, retrying according to the configured [`RetryPolicy`].
    ///
    /// Transport errors are returned immediately. Requests with streaming
    /// bodies cannot be cloned and are sent exactly once.
    async fn send(&self, request: reqwest::Request) -> Result<reqwest::Response, BmcError> {
        let Some(policy) = &self.retry else {
            return Ok(self.inner.execute(request).await?);
        };

        let mut attempt: u32 = 0;
        let mut current = request;
        loop {
            let is_last = attempt >= policy.max_retries;
            // try_clone() returns None for streaming bodies, which therefore
            // get a single attempt.
            let next = if is_last { None } else { current.try_clone() };
            let response = self.inner.execute(current).await?;
            match next {
                // The clone is identical to the request just sent, so the
                // classifier sees what went over the wire.
                Some(next_request) if (policy.classifier)(&next_request, &response) => {
                    if let Some(delay) = policy.delay {
                        sleep(delay).await;
                    }
                    current = next_request;
                    attempt += 1;
                }
                _ => return Ok(response),
            }
        }
    }

    async fn handle_response<T>(&self, response: reqwest::Response) -> Result<T, BmcError>
    where
        T: DeserializeOwned,
    {
        if !response.status().is_success() {
            return Err(BmcError::InvalidResponse {
                url: response.url().clone(),
                status: response.status(),
                text: response.text().await.unwrap_or_else(|_| "<no data>".into()),
            });
        }

        let headers = response.headers().clone();

        let etag_header = etag_from_headers(&headers);

        let mut value: serde_json::Value = response.json().await.map_err(BmcError::ReqwestError)?;

        if let Some(etag) = etag_header {
            inject_etag(&etag, &mut value);
        }

        serde_path_to_error::deserialize(value).map_err(BmcError::JsonError)
    }

    async fn handle_modification_response<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<ModificationResponse<T>, BmcError>
    where
        T: DeserializeOwned + Send + Sync,
    {
        let status = response.status();
        let url = response.url().clone();
        let headers = response.headers().clone();
        if !status.is_success() {
            return Err(BmcError::InvalidResponse {
                url,
                status,
                text: response.text().await.unwrap_or_else(|_| "<no data>".into()),
            });
        }

        let etag = etag_from_headers(&headers);

        // Resolve the header once, but defer propagating its error until a
        // status branch actually uses Location. A malformed, irrelevant
        // Location must not turn a valid 204 or body-bearing 200/201 into an
        // error.
        let location = location_from_headers(&headers, &url, status);

        match status {
            reqwest::StatusCode::NO_CONTENT => Ok(ModificationResponse::Empty),
            reqwest::StatusCode::ACCEPTED => {
                let Some(task_location) = location? else {
                    return Err(BmcError::InvalidResponse {
                        url,
                        status,
                        text: String::from("202 Accepted without Location header"),
                    });
                };

                Ok(ModificationResponse::Task(AsyncTask {
                    location: task_location.into(),
                    retry_after: retry_after_from_headers(&headers),
                }))
            }
            reqwest::StatusCode::OK | reqwest::StatusCode::CREATED => {
                let bytes = response.bytes().await.map_err(BmcError::ReqwestError)?;
                if !bytes.is_empty() {
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes).map_err(BmcError::DecodeError)?;
                    let mut value = value;

                    if value.get("@odata.id").is_some() {
                        if let Some(etag) = etag {
                            inject_etag(&etag, &mut value);
                        }
                    }

                    // Non-empty 200/201 bodies are typed responses selected by the caller.
                    //
                    // - These bodies are not required to be Redfish resources with @odata.id.
                    //
                    // - DSP0266 POST actions with no response body may still return "an error
                    //   response, with a message that indicates success".
                    return match serde_path_to_error::deserialize(&value) {
                        // Non-empty 200/201 body matched the caller-selected type.
                        Ok(entity) => Ok(ModificationResponse::Entity(entity)),
                        Err(err) => {
                            if is_redfish_success_response(&value) {
                                // No-response action returned a Redfish success envelope.
                                Ok(ModificationResponse::Empty)
                            } else {
                                // The response was successful JSON, but it did
                                // not match the caller-selected response type.
                                Err(BmcError::JsonError(err))
                            }
                        }
                    };
                }

                if let Some(location) = location? {
                    let value = serde_json::json!({ "@odata.id": location });
                    return serde_path_to_error::deserialize(value)
                        .map(ModificationResponse::Entity)
                        .map_err(BmcError::JsonError);
                }

                Ok(ModificationResponse::Empty)
            }
            _ => Err(BmcError::InvalidResponse {
                url,
                status,
                text: format!("Unexpected successful status code: {status}"),
            }),
        }
    }

    async fn handle_session_response<T>(
        &self,
        response: reqwest::Response,
    ) -> Result<SessionCreateResponse<T>, BmcError>
    where
        T: DeserializeOwned + Send + Sync,
    {
        let status = response.status();
        let url = response.url().clone();
        let headers = response.headers().clone();
        if !status.is_success() {
            return Err(BmcError::InvalidResponse {
                url,
                status,
                text: response.text().await.unwrap_or_else(|_| "<no data>".into()),
            });
        }

        let Some(auth_token) = auth_token_from_headers(&headers) else {
            return Err(BmcError::InvalidResponse {
                url,
                status,
                text: String::from("session creation response missing X-Auth-Token header"),
            });
        };

        // The returned location is the durable session identifier used for
        // later deletion, so normalize and validate it before exposing it.
        let Some(location) = location_from_headers(&headers, &url, status)? else {
            return Err(BmcError::InvalidResponse {
                url,
                status,
                text: String::from("session creation response missing Location header"),
            });
        };

        match status {
            reqwest::StatusCode::OK | reqwest::StatusCode::CREATED => {
                let etag = etag_from_headers(&headers);
                let bytes = response.bytes().await.map_err(BmcError::ReqwestError)?;
                if bytes.is_empty() {
                    return Err(BmcError::InvalidResponse {
                        url,
                        status,
                        text: String::from("session creation response missing entity body"),
                    });
                }

                let mut value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(BmcError::DecodeError)?;
                if let Some(etag) = etag {
                    inject_etag(&etag, &mut value);
                }
                let entity =
                    serde_path_to_error::deserialize(value).map_err(BmcError::JsonError)?;

                Ok(SessionCreateResponse {
                    entity,
                    auth_token,
                    location,
                })
            }
            reqwest::StatusCode::ACCEPTED => Err(BmcError::InvalidResponse {
                url,
                status,
                text: String::from("session creation returned 202 Accepted without session entity"),
            }),
            reqwest::StatusCode::NO_CONTENT => Err(BmcError::InvalidResponse {
                url,
                status,
                text: String::from("session creation returned 204 No Content"),
            }),
            _ => Err(BmcError::InvalidResponse {
                url,
                status,
                text: format!("Unexpected successful status code for session creation: {status}"),
            }),
        }
    }
}

/// Wraps a redirect policy to reject cross-origin targets.
fn same_origin_redirect_policy(redirect_policy: RedirectPolicy) -> RedirectPolicy {
    RedirectPolicy::custom(move |attempt| {
        let Some(original_url) = attempt.previous().first() else {
            return attempt.error("redirect attempt is missing the original URL");
        };

        if attempt.url().origin() != original_url.origin() {
            return attempt.error("cross-origin redirects are not allowed");
        }

        redirect_policy.redirect(attempt)
    })
}

/// Resolve a Redfish `Location` header into a same-origin path and query.
///
/// HTTP defines `Location` as a URI reference, so values may be absolute,
/// root-relative, path-relative, or query-only. Resolution must use the final
/// response URL; treating a relative value as a path rooted at the configured
/// BMC endpoint can target a different resource. The returned `ODataId` keeps
/// only path and query because fragments are not sent in subsequent HTTP
/// requests and transport always uses the configured BMC origin.
fn location_from_headers(
    headers: &HeaderMap,
    response_url: &Url,
    status: reqwest::StatusCode,
) -> Result<Option<ODataId>, BmcError> {
    let invalid_response = |text: &'static str| BmcError::InvalidResponse {
        url: response_url.clone(),
        status,
        text: text.to_string(),
    };

    let Some(value) = headers.get(header::LOCATION) else {
        return Ok(None);
    };

    let raw = value
        .to_str()
        .map_err(|_| invalid_response("Location header is not valid text"))?;

    let raw = raw.trim();

    // Joining either value would resolve back to the response resource, which
    // cannot identify a newly created session or asynchronous task monitor.
    if raw.is_empty() || raw.starts_with('#') {
        return Err(invalid_response(
            "Location header does not identify a resource",
        ));
    }

    let resolved = response_url
        .join(raw)
        .map_err(|_| invalid_response("Location header is not a valid URI reference"))?;

    // Redfish follow-up requests carry BMC credentials. Reject another origin
    // before reducing the URL to an OData path and losing that distinction.
    if resolved.origin() != response_url.origin() {
        return Err(invalid_response(
            "Location header resolves to a different origin",
        ));
    }

    let mut path_and_query = resolved.path().to_string();

    // Preserve the query separately from the path so later polling or deletion
    // sends it as a query instead of percent-encoded path text.
    if let Some(query) = resolved.query() {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }

    Ok(Some(path_and_query.into()))
}

fn auth_token_from_headers(headers: &HeaderMap) -> Option<String> {
    headers
        .get("x-auth-token")
        .and_then(|value| value.to_str().ok())
        .map(ToString::to_string)
}

fn etag_from_headers(headers: &HeaderMap) -> Option<ODataETag> {
    headers
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(|v| v.to_string().into())
}

fn retry_after_from_headers(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        // RFC 9110 defines the numeric Retry-After form as delay-seconds.
        // This helper handles that form and leaves HTTP-date support out of scope.
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

fn inject_etag(etag: &ODataETag, body: &mut serde_json::Value) {
    if let Some(obj) = body.as_object_mut() {
        let etag_value = serde_json::Value::String(etag.to_string());

        // Handles both absent and null values
        obj.entry("@odata.etag")
            .and_modify(|v| *v = etag_value.clone())
            .or_insert(etag_value);
    }
}

/// DSP0266 7.11, Table 10 allows actions without response bodies to return
/// an error-shaped success body. Only that body should become Empty.
#[inline]
fn is_redfish_success_response(value: &serde_json::Value) -> bool {
    #[derive(serde::Deserialize)]
    struct ExtendedInfoEnvelope {
        #[serde(rename = "@Message.ExtendedInfo")]
        _extended_info: Vec<Message>,
    }

    // If we recieved extended info, this means we got a success response
    if <ExtendedInfoEnvelope as serde::Deserialize>::deserialize(value).is_ok() {
        return true;
    }

    let Ok(response) = <RedfishError as serde::Deserialize>::deserialize(value) else {
        return false;
    };

    let code = response.error.code.as_str();
    let message = code.rsplit_once('.').map_or(code, |(_, message)| message);

    matches!(message, "Success" | "Created" | "NoOperation")
}

fn auth_headers(
    request: reqwest::RequestBuilder,
    credentials: &BmcCredentials,
) -> reqwest::RequestBuilder {
    match credentials {
        BmcCredentials::UsernamePassword { username, password } => {
            request.basic_auth(username, password.as_ref())
        }
        BmcCredentials::Token { token } => request.header("X-Auth-Token", token),
    }
}

impl HttpClient for Client {
    type Error = BmcError;

    async fn get<T>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        etag: Option<ODataETag>,
        custom_headers: &HeaderMap,
    ) -> Result<T, Self::Error>
    where
        T: DeserializeOwned,
    {
        let mut request =
            auth_headers(self.inner.get(url), credentials).headers(custom_headers.clone());

        if let Some(etag) = etag {
            request = request.header(header::IF_NONE_MATCH, etag.to_string());
        }

        let response = self.send(request.build()?).await?;
        self.handle_response(response).await
    }

    async fn post<B, T>(
        &self,
        url: Url,
        body: &B,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> Result<ModificationResponse<T>, Self::Error>
    where
        B: Serialize + Send + Sync,
        T: DeserializeOwned + Send + Sync,
    {
        let request = auth_headers(self.inner.post(url), credentials)
            .headers(custom_headers.clone())
            .json(body);

        let response = self.send(request.build()?).await?;
        self.handle_modification_response(response).await
    }

    async fn post_session<B, T>(
        &self,
        url: Url,
        body: &B,
        custom_headers: &HeaderMap,
    ) -> Result<SessionCreateResponse<T>, Self::Error>
    where
        B: Serialize + Send + Sync,
        T: DeserializeOwned + Send + Sync,
    {
        let request = self
            .inner
            .post(url)
            .headers(custom_headers.clone())
            .json(body);

        let response = self.send(request.build()?).await?;
        self.handle_session_response(response).await
    }

    async fn patch<B, T>(
        &self,
        url: Url,
        etag: ODataETag,
        body: &B,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> Result<ModificationResponse<T>, Self::Error>
    where
        B: Serialize + Send + Sync,
        T: DeserializeOwned + Send + Sync,
    {
        let mut request =
            auth_headers(self.inner.patch(url), credentials).headers(custom_headers.clone());

        request = request.header(header::IF_MATCH, etag.to_string());

        let response = self.send(request.json(body).build()?).await?;
        self.handle_modification_response(response).await
    }

    async fn delete<T>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> Result<ModificationResponse<T>, Self::Error>
    where
        T: DeserializeOwned + Send + Sync,
    {
        let request =
            auth_headers(self.inner.delete(url), credentials).headers(custom_headers.clone());

        let response = self.send(request.build()?).await?;
        self.handle_modification_response(response).await
    }

    async fn post_multipart_update<U, V, T>(
        &self,
        url: Url,
        update_request: MultipartUpdateRequest<'_, U, V>,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> Result<ModificationResponse<T>, Self::Error>
    where
        U: UploadReader,
        T: DeserializeOwned + Send + Sync,
        V: Serialize + Send + Sync,
    {
        let MultipartUpdateRequest {
            update_parameters,
            update_stream,
            oem_parts,
            upload_timeout,
        } = update_request;

        // First, check if all OEM parts have valid names.
        for part in &oem_parts {
            if !part.is_name_valid() {
                return Err(BmcError::InvalidRequest(format!(
                    "OEM part's name `{}` is invalid",
                    part.name
                )));
            }
        }

        let stream_part = build_stream_part(update_stream, "application/octet-stream")?;
        let update_parameters_part = build_update_parameters_part(update_parameters)?;

        let mut form = Form::new()
            .part("UpdateParameters", update_parameters_part)
            .part("UpdateFile", stream_part);

        for part in oem_parts {
            let (name, part) = build_oem_part(part)?;
            form = form.part(name, part);
        }

        let request = auth_headers(self.inner.post(url), credentials)
            .headers(custom_headers.clone())
            .multipart(form)
            .timeout(upload_timeout);

        let response = self.send(request.build()?).await?;
        self.handle_modification_response(response).await
    }

    #[cfg(feature = "update-service-deprecated")]
    async fn post_http_push_uri_update<U, T>(
        &self,
        url: Url,
        update_request: HttpPushUriUpdateRequest<U>,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> Result<ModificationResponse<T>, Self::Error>
    where
        U: UploadReader,
        T: DeserializeOwned + Send + Sync,
    {
        let HttpPushUriUpdateRequest {
            update_stream,
            upload_timeout,
        } = update_request;

        let UploadStream {
            reader,
            content_length,
        } = update_stream;

        let body = reqwest::Body::wrap_stream(ReaderStream::new(reader.compat()));

        let mut request = auth_headers(self.inner.post(url), credentials)
            .headers(custom_headers.clone())
            .header(header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .timeout(upload_timeout);

        if let Some(content_length) = content_length {
            request = request.header(header::CONTENT_LENGTH, content_length.to_string());
        }

        let response = self.send(request.build()?).await?;
        self.handle_modification_response(response).await
    }

    // The idle branch matches on `Option<Duration>` where both arms await
    // distinct future types, which cannot be expressed as an `Option` combinator
    // without boxing; `option_if_let_else`'s suggestion does not apply.
    #[allow(clippy::option_if_let_else)]
    async fn sse<T: Send + Sized + for<'de> serde::Deserialize<'de>>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> Result<BoxTryStream<T, Self::Error>, Self::Error> {
        let request = auth_headers(self.inner.get(url), credentials)
            .headers(custom_headers.clone())
            .header(header::ACCEPT, "text/event-stream")
            .timeout(Duration::MAX);

        let response = self.send(request.build()?).await?;

        if !response.status().is_success() {
            return Err(BmcError::InvalidResponse {
                url: response.url().clone(),
                status: response.status(),
                text: response.text().await.unwrap_or_else(|_| "<no data>".into()),
            });
        }

        let capped = cap_event_bytes(response.bytes_stream(), self.sse.max_event_bytes);
        let events = sse_stream::SseStream::from_bytes_stream(capped)
            .filter_map(|event| async move { event_to_item::<T>(event) });

        // Liveness bound: optionally abort if no event arrives within the idle
        // window. It is keyed on decoded events, so comment heartbeats (which
        // `sse_stream` discards) do NOT reset it; a server relying solely on
        // comment heartbeats should not enable the idle timeout.
        let idle = self.sse.idle_timeout;
        let guarded = unfold(
            (Box::pin(events), false),
            move |(mut events, done)| async move {
                if done {
                    return None;
                }
                let next = match idle {
                    Some(d) => match timeout(d, events.next()).await {
                        Ok(item) => item,
                        Err(_) => Some(Err(BmcError::SseIdleTimeout { idle: d })),
                    },
                    None => events.next().await,
                };
                match next {
                    Some(Err(e)) => {
                        let terminal = e.is_stream_fatal();
                        Some((Err(e), (events, terminal)))
                    }
                    Some(ok) => Some((ok, (events, false))),
                    None => None,
                }
            },
        );

        Ok(Box::pin(guarded))
    }
}

fn build_update_parameters_part<V>(update_parameters: &V) -> Result<Part, BmcError>
where
    V: Serialize + Send + Sync,
{
    Part::bytes(serde_json::to_vec(update_parameters).map_err(BmcError::EncodeError)?)
        .mime_str("application/json")
        .map_err(BmcError::ReqwestError)
}

fn build_stream_part<U>(stream: DataStream<U>, content_type: &'static str) -> Result<Part, BmcError>
where
    U: UploadReader,
{
    let DataStream {
        name,
        reader,
        content_length,
    } = stream;

    let body = reqwest::Body::wrap_stream(ReaderStream::new(reader.compat()));
    let part = match content_length {
        Some(length) => Part::stream_with_length(body, length),
        None => Part::stream(body),
    };

    part.file_name(name)
        .mime_str(content_type)
        .map_err(BmcError::ReqwestError)
}

fn build_oem_part(part: OemMultipartPart) -> Result<(String, Part), BmcError> {
    let OemMultipartPart {
        name,
        reader,
        content_type,
        content_length,
    } = part;

    let body = reqwest::Body::wrap_stream(ReaderStream::new(reader.compat()));

    let mut part = match content_length {
        Some(length) => Part::stream_with_length(body, length),
        None => Part::stream(body),
    };

    if let Some(content_type) = content_type {
        part = part.mime_str(&content_type)?;
    }

    Ok((name, part))
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::*;

    use futures_util::io::Cursor;
    use http::HeaderValue;
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;
    use wiremock::Mock;
    use wiremock::MockBuilder;
    use wiremock::MockServer;
    use wiremock::Request;
    use wiremock::ResponseTemplate;

    #[derive(serde::Serialize)]
    struct MultipartParameters {
        #[serde(rename = "ForceUpdate")]
        force_update: bool,

        #[serde(rename = "Targets")]
        targets: Vec<String>,
    }

    fn session_auth() -> (BmcCredentials, HeaderMap) {
        let credentials = BmcCredentials::token("session-secret".to_string());
        let mut headers = HeaderMap::new();

        headers.insert(
            "X-Custom-Secret",
            http::HeaderValue::from_static("custom-secret"),
        );

        (credentials, headers)
    }

    fn credentialed_get(resource_path: &str) -> MockBuilder {
        Mock::given(method("GET"))
            .and(path(resource_path))
            .and(header("X-Auth-Token", "session-secret"))
            .and(header("X-Custom-Secret", "custom-secret"))
    }

    async fn assert_complete_event_honors_byte_limit(chunks: &[&'static [u8]]) {
        let event_len = chunks.iter().map(|chunk| chunk.len()).sum::<usize>();

        let exact = futures_util::stream::iter(
            chunks
                .iter()
                .copied()
                .map(|chunk| Ok::<_, reqwest::Error>(Bytes::from_static(chunk))),
        );

        let exact_results = cap_event_bytes(exact, event_len).collect::<Vec<_>>().await;

        assert_eq!(exact_results.len(), chunks.len());
        assert!(exact_results.iter().all(Result::is_ok));

        let oversized = futures_util::stream::iter(
            chunks
                .iter()
                .copied()
                .map(|chunk| Ok::<_, reqwest::Error>(Bytes::from_static(chunk))),
        );

        let oversized_results = cap_event_bytes(oversized, event_len - 1)
            .collect::<Vec<_>>()
            .await;

        assert!(matches!(
            oversized_results.last(),
            Some(Err(SseByteError::EventTooLarge { limit })) if *limit == event_len - 1
        ));
    }

    async fn mount_redirect(mock_server: &MockServer, source_path: &str, location: &str) {
        Mock::given(method("GET"))
            .and(path(source_path))
            .respond_with(ResponseTemplate::new(302).insert_header("Location", location))
            .expect(1)
            .mount(mock_server)
            .await;
    }

    #[tokio::test]
    async fn complete_events_honor_byte_limit_across_line_endings() {
        const LF_EVENT: &[u8] = b"data: {}\n\n";
        const CR_EVENT: &[u8] = b"data: {}\r\r";
        const CRLF_EVENT: &[u8] = b"data: {}\r\n\r\n";

        assert_complete_event_honors_byte_limit(&[LF_EVENT]).await;
        assert_complete_event_honors_byte_limit(&[CR_EVENT]).await;
        assert_complete_event_honors_byte_limit(&[CRLF_EVENT]).await;

        let cr_split_at = CR_EVENT.len() - 1;

        assert_complete_event_honors_byte_limit(&[
            &CR_EVENT[..cr_split_at],
            &CR_EVENT[cr_split_at..],
        ])
        .await;

        let crlf_split_at = CRLF_EVENT.len() - 1;

        assert_complete_event_honors_byte_limit(&[
            &CRLF_EVENT[..crlf_split_at],
            &CRLF_EVENT[crlf_split_at..],
        ])
        .await;
    }

    #[test]
    fn test_cacheable_error_trait() {
        let mock_response = reqwest::Response::from(
            http::Response::builder()
                .status(304)
                .body("")
                .expect("Valid empty body"),
        );
        let error = BmcError::InvalidResponse {
            url: "http://example.com/redfish/v1".parse().unwrap(),
            status: mock_response.status(),
            text: "".into(),
        };
        assert!(error.is_cached());

        let cache_miss = BmcError::CacheMiss;
        assert!(!cache_miss.is_cached());

        let created_miss = BmcError::cache_miss();
        assert!(matches!(created_miss, BmcError::CacheMiss));
    }

    #[test]
    fn action_error_classifies_only_not_found_responses() {
        let url: Url = "http://example.com/redfish/v1/Actions/Test"
            .parse()
            .expect("valid URL");

        for (status, expected) in [
            (reqwest::StatusCode::NOT_FOUND, true),
            (reqwest::StatusCode::INTERNAL_SERVER_ERROR, false),
        ] {
            let error = BmcError::InvalidResponse {
                url: url.clone(),
                status,
                text: String::new(),
            };

            assert_eq!(error.is_not_found(), expected);
        }

        assert!(!BmcError::CacheMiss.is_not_found());
    }

    #[tokio::test]
    async fn cross_origin_redirect_is_rejected_before_forwarding_credentials(
    ) -> Result<(), Box<dyn StdError>> {
        let source_server = MockServer::start().await;
        let destination_server = MockServer::start().await;
        let source_path = "/redfish/v1";
        let destination_path = "/capture";
        let destination_url = format!("{}{destination_path}", destination_server.uri());

        mount_redirect(&source_server, source_path, &destination_url).await;

        credentialed_get(destination_path)
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({})))
            .expect(0)
            .mount(&destination_server)
            .await;

        let client = Client::new()?;
        let (credentials, headers) = session_auth();

        let response = client
            .get::<serde_json::Value>(
                Url::parse(&format!("{}{source_path}", source_server.uri()))?,
                &credentials,
                None,
                &headers,
            )
            .await;

        assert!(matches!(
            response,
            Err(BmcError::ReqwestError(error)) if error.is_redirect()
        ));

        destination_server.verify().await;

        Ok(())
    }

    #[tokio::test]
    async fn same_origin_redirect_with_reqwest_default_policy_preserves_credentials(
    ) -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let source_path = "/redfish/v1";
        let destination_path = "/redfish/v1/redirected";

        mount_redirect(&mock_server, source_path, destination_path).await;

        credentialed_get(destination_path)
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({ "redirected": true })),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let mut params = ClientParams::new();
        params.max_redirects = None;

        let client = Client::with_params(params)?;
        let (credentials, headers) = session_auth();

        let response: serde_json::Value = client
            .get(
                Url::parse(&format!("{}{source_path}", mock_server.uri()))?,
                &credentials,
                None,
                &headers,
            )
            .await?;

        assert_eq!(response["redirected"], true);
        mock_server.verify().await;

        Ok(())
    }

    #[test]
    fn location_header_resolves_valid_and_rejects_invalid_references(
    ) -> Result<(), Box<dyn StdError>> {
        struct TestCase(&'static str, Result<&'static str, &'static str>);

        let response_url = Url::parse("https://bmc.example/redfish/v1/SessionService/Sessions")?;

        let cases = [
            TestCase(
                "https://bmc.example/redfish/v1/SessionService/Sessions/1?token=abc",
                Ok("/redfish/v1/SessionService/Sessions/1?token=abc"),
            ),
            TestCase(
                "/redfish/v1/SessionService/Sessions/1?token=abc",
                Ok("/redfish/v1/SessionService/Sessions/1?token=abc"),
            ),
            TestCase(
                "Sessions/1?token=abc",
                Ok("/redfish/v1/SessionService/Sessions/1?token=abc"),
            ),
            TestCase(
                "../TaskService/Tasks/42?token=abc",
                Ok("/redfish/v1/TaskService/Tasks/42?token=abc"),
            ),
            TestCase(
                "?token=abc",
                Ok("/redfish/v1/SessionService/Sessions?token=abc"),
            ),
            TestCase(
                "//bmc.example/redfish/v1/TaskService/Tasks/42",
                Ok("/redfish/v1/TaskService/Tasks/42"),
            ),
            TestCase("", Err("Location header does not identify a resource")),
            TestCase(
                "#fragment",
                Err("Location header does not identify a resource"),
            ),
            TestCase(
                "https://other.example/redfish/v1/TaskService/Tasks/42",
                Err("Location header resolves to a different origin"),
            ),
            TestCase(
                "//bmc.example:8443/redfish/v1/TaskService/Tasks/42",
                Err("Location header resolves to a different origin"),
            ),
            TestCase(
                "//host:99999/path",
                Err("Location header is not a valid URI reference"),
            ),
        ];

        for TestCase(raw, expected) in cases {
            let mut headers = HeaderMap::new();
            headers.insert(header::LOCATION, raw.parse::<HeaderValue>()?);

            let result =
                location_from_headers(&headers, &response_url, reqwest::StatusCode::CREATED);

            match (result, expected) {
                (Ok(Some(location)), Ok(expected)) => {
                    assert_eq!(location.to_string(), expected, "{raw}");
                }
                (Err(BmcError::InvalidResponse { text, .. }), Err(expected)) => {
                    assert_eq!(text, expected, "{raw}");
                }
                _ => return Err(format!("{raw}: unexpected Location result").into()),
            }
        }

        Ok(())
    }

    /// Retry policy used in tests: retries GET requests on 503 responses.
    fn test_retry_policy(max_retries: u32, delay: Option<Duration>) -> RetryPolicy {
        let policy = RetryPolicy::new(|request, response| {
            *request.method() == http::Method::GET
                && response.status() == http::StatusCode::SERVICE_UNAVAILABLE
        })
        .max_retries(max_retries);
        match delay {
            Some(delay) => policy.delay(delay),
            None => policy,
        }
    }

    /// Mounts mocks that respond with `unavailable` 503s followed by a 200.
    async fn mount_unavailable_then_ok(
        mock_server: &MockServer,
        resource_path: &str,
        unavailable: u64,
    ) {
        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(unavailable)
            .expect(unavailable)
            .mount(mock_server)
            .await;

        Mock::given(method("GET"))
            .and(path(resource_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({ "@odata.id": resource_path })),
            )
            .expect(1)
            .mount(mock_server)
            .await;
    }

    #[tokio::test]
    async fn test_get_is_retried_on_retryable_status() -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1";
        mount_unavailable_then_ok(&mock_server, resource_path, 2).await;

        let client = Client::with_params(ClientParams::new().retry(test_retry_policy(2, None)))?;
        let credentials = BmcCredentials::new("root".to_string(), "password".to_string());

        let response: serde_json::Value = client
            .get(
                Url::parse(&format!("{}{resource_path}", mock_server.uri()))?,
                &credentials,
                None,
                &HeaderMap::new(),
            )
            .await?;

        assert_eq!(response["@odata.id"], resource_path);

        Ok(())
    }

    #[tokio::test]
    async fn test_post_is_not_retried() -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset";

        // The policy retries 503s, but only for GET requests. A POST returning
        // 503 must therefore reach the server exactly once.
        Mock::given(method("POST"))
            .and(path(resource_path))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&mock_server)
            .await;

        let client = Client::with_params(ClientParams::new().retry(test_retry_policy(3, None)))?;
        let credentials = BmcCredentials::new("root".to_string(), "password".to_string());

        let response = client
            .post::<_, serde_json::Value>(
                Url::parse(&format!("{}{resource_path}", mock_server.uri()))?,
                &serde_json::json!({ "ResetType": "ForceRestart" }),
                &credentials,
                &HeaderMap::new(),
            )
            .await;

        assert!(matches!(
            response,
            Err(BmcError::InvalidResponse { status, .. })
                if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));

        Ok(())
    }

    #[tokio::test]
    async fn test_retry_delay_is_observed() -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let resource_path = "/redfish/v1";
        mount_unavailable_then_ok(&mock_server, resource_path, 2).await;

        let delay = Duration::from_millis(100);
        let client =
            Client::with_params(ClientParams::new().retry(test_retry_policy(2, Some(delay))))?;
        let credentials = BmcCredentials::new("root".to_string(), "password".to_string());

        let started = std::time::Instant::now();
        let response: serde_json::Value = client
            .get(
                Url::parse(&format!("{}{resource_path}", mock_server.uri()))?,
                &credentials,
                None,
                &HeaderMap::new(),
            )
            .await?;

        // Two retries mean two sleeps; only assert the lower bound to keep
        // the test robust on slow CI machines.
        assert!(started.elapsed() >= Duration::from_millis(180));
        assert_eq!(response["@odata.id"], resource_path);

        Ok(())
    }

    #[tokio::test]
    async fn test_streaming_body_is_not_retried() -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update-multipart";

        // Exactly one request must reach the server even though the policy
        // retries every 503: try_clone() returns None for streaming bodies,
        // which therefore get a single attempt.
        Mock::given(method("POST"))
            .and(path(upload_path))
            .respond_with(ResponseTemplate::new(503))
            .expect(1)
            .mount(&mock_server)
            .await;

        let policy = RetryPolicy::new(|_request, response| {
            response.status() == http::StatusCode::SERVICE_UNAVAILABLE
        })
        .max_retries(3);
        let client = Client::with_params(ClientParams::new().retry(policy))?;
        let credentials = BmcCredentials::new("root".to_string(), "password".to_string());

        let params = MultipartParameters {
            force_update: true,
            targets: vec!["/redfish/v1/Systems/1".to_string()],
        };

        let update_stream =
            DataStream::new("firmware.bin", Cursor::new(b"firmware-bytes".to_vec()))
                .with_content_length(14);

        let update_request = MultipartUpdateRequest {
            update_parameters: &params,
            update_stream,
            oem_parts: vec![],
            upload_timeout: Duration::from_secs(600),
        };

        let response = client
            .post_multipart_update::<_, _, serde_json::Value>(
                Url::parse(&format!("{}{upload_path}", mock_server.uri()))?,
                update_request,
                &credentials,
                &HeaderMap::new(),
            )
            .await;

        assert!(matches!(
            response,
            Err(BmcError::InvalidResponse { status, .. })
                if status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));

        Ok(())
    }

    #[tokio::test]
    async fn test_multipart_form_fails_oem_validation() -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update-multipart";
        let task_path = "/redfish/v1/TaskService/Tasks/42";

        Mock::given(method("POST"))
            .and(path(upload_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("X-Upload-Mode", "multipart"))
            .and(|request: &Request| {
                multipart_body_contains(request, "firmware.bin", "firmware-bytes")
            })
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("Location", format!("{}{task_path}", mock_server.uri()))
                    .insert_header("Retry-After", "15"),
            )
            .expect(0)
            .mount(&mock_server)
            .await;

        let params = MultipartParameters {
            force_update: true,
            targets: vec!["/redfish/v1/Systems/1".to_string()],
        };

        let mut custom_headers = HeaderMap::new();
        custom_headers.insert("X-Upload-Mode", http::HeaderValue::from_static("multipart"));

        let client = Client::new()?;
        let credentials = BmcCredentials::new("root".to_string(), "password".to_string());

        //
        // Invalid OEM part.
        //
        let update_stream =
            DataStream::new("firmware.bin", Cursor::new(b"firmware-bytes".to_vec()))
                .with_content_length(14);

        // Construction error - fails name validation.
        let r = OemMultipartPart::new("oemNvidia", Cursor::new(br#"{"Mode":"Rms"}"#.to_vec()));
        assert!(r.is_err());

        let mut invalid_oem_part =
            OemMultipartPart::new("OemNvidia", Cursor::new(br#"{"Mode":"Rms"}"#.to_vec()))?
                .with_content_type("application/json");
        invalid_oem_part.name = "oemNvidia".to_string();

        let update_request = MultipartUpdateRequest {
            update_parameters: &params,
            update_stream,
            oem_parts: vec![invalid_oem_part],
            upload_timeout: Duration::from_secs(600),
        };

        let response = client
            .post_multipart_update::<_, _, serde_json::Value>(
                Url::parse(&format!("{}{upload_path}", mock_server.uri()))?,
                update_request,
                &credentials,
                &custom_headers,
            )
            .await;

        assert!(response.is_err());

        Ok(())
    }

    #[tokio::test]
    async fn test_multipart_form_sends_parts_and_returns_task() -> Result<(), Box<dyn StdError>> {
        let mock_server = MockServer::start().await;
        let upload_path = "/redfish/v1/UpdateService/update-multipart";
        let task_path = "/redfish/v1/TaskService/Tasks/42";

        Mock::given(method("POST"))
            .and(path(upload_path))
            .and(header("authorization", "Basic cm9vdDpwYXNzd29yZA=="))
            .and(header("X-Upload-Mode", "multipart"))
            .and(|request: &Request| {
                multipart_body_contains(request, "firmware.bin", "firmware-bytes")
            })
            .respond_with(
                ResponseTemplate::new(202)
                    .insert_header("Location", format!("{}{task_path}", mock_server.uri()))
                    .insert_header("Retry-After", "15"),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let params = MultipartParameters {
            force_update: true,
            targets: vec!["/redfish/v1/Systems/1".to_string()],
        };

        let mut custom_headers = HeaderMap::new();
        custom_headers.insert("X-Upload-Mode", http::HeaderValue::from_static("multipart"));

        let client = Client::new()?;
        let credentials = BmcCredentials::new("root".to_string(), "password".to_string());

        let update_stream =
            DataStream::new("firmware.bin", Cursor::new(b"firmware-bytes".to_vec()))
                .with_content_length(14);

        let update_request = MultipartUpdateRequest {
            update_parameters: &params,
            update_stream,
            oem_parts: vec![OemMultipartPart::new(
                "OemNvidia",
                Cursor::new(br#"{"Mode":"Rms"}"#.to_vec()),
            )?
            .with_content_type("application/json")],
            upload_timeout: Duration::from_secs(600),
        };

        let response = client
            .post_multipart_update::<_, _, serde_json::Value>(
                Url::parse(&format!("{}{upload_path}", mock_server.uri()))?,
                update_request,
                &credentials,
                &custom_headers,
            )
            .await?;

        let ModificationResponse::Task(task) = response else {
            return Err(String::from("expected task response").into());
        };

        assert_eq!(task.location.0.to_string(), task_path);
        assert_eq!(task.retry_after, Some(Duration::from_secs(15)));

        Ok(())
    }

    fn multipart_body_contains(request: &Request, file_name: &str, file_body: &str) -> bool {
        let Some(content_type) = request
            .headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
        else {
            return false;
        };

        let body = String::from_utf8_lossy(&request.body);

        content_type.starts_with("multipart/form-data; boundary=")
            && body.contains("name=\"UpdateParameters\"")
            && body.contains("Content-Type: application/json")
            && body.contains("\"ForceUpdate\":true")
            && body.contains("\"Targets\":[\"/redfish/v1/Systems/1\"]")
            && body.contains("name=\"UpdateFile\"")
            && body.contains("Content-Type: application/octet-stream")
            && body.contains(&format!("filename=\"{file_name}\""))
            && body.contains("name=\"OemNvidia\"")
            && !body.contains("name=\"OemNvidia\"; filename=")
            && body.contains("{\"Mode\":\"Rms\"}")
            && body.contains(file_body)
    }
}
