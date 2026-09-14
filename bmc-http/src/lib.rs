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

#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::suspicious,
    clippy::complexity,
    clippy::perf
)]
#![deny(
    clippy::absolute_paths,
    clippy::todo,
    clippy::unimplemented,
    clippy::tests_outside_test_module,
    clippy::panic,
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    clippy::unused_trait_names,
    clippy::print_stdout,
    clippy::print_stderr
)]
#![allow(clippy::doc_markdown)]
#![deny(missing_docs)]

//! HTTP implementation of [`nv_redfish_core::Bmc`] trait.

pub mod cache;
pub mod credentials;

#[cfg(feature = "http-extras")]
mod concurrency;
#[cfg(feature = "reqwest")]
mod schema;

#[cfg(feature = "reqwest")]
pub mod reqwest;

use std::error::Error as StdError;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::sync::RwLock;

use crate::cache::TypeErasedCarCache;

use http::HeaderMap;
use nv_redfish_core::query::ExpandQuery;
use nv_redfish_core::without_event_ids;
use nv_redfish_core::Action;
use nv_redfish_core::Bmc;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
use nv_redfish_core::FilterQuery;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::SessionCreateResponse;
use nv_redfish_core::StreamEvent;
use nv_redfish_core::UploadReader;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use url::Url;

#[cfg(feature = "http-extras")]
pub use concurrency::ConcurrencyLimitedBmc;
#[doc(inline)]
pub use credentials::BmcCredentials;

#[cfg(feature = "update-service-deprecated")]
#[doc(inline)]
pub use nv_redfish_core::HttpPushUriUpdateRequest;
#[cfg(feature = "update-service-deprecated")]
#[doc(inline)]
pub use nv_redfish_core::UploadStream;

#[doc(inline)]
pub use nv_redfish_core::MultipartUpdateRequest;

/// HTTP Client trait.
///
/// nv-redfish-bmc-http supports any HTTP implementation that
/// implements this [`HttpClient`] trait.
pub trait HttpClient: Send + Sync {
    /// HTTP client error.
    type Error: Send + StdError;

    /// Perform an HTTP GET request with optional conditional headers.
    fn get<T>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        etag: Option<ODataETag>,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<T, Self::Error>> + Send
    where
        T: DeserializeOwned + Send + Sync;

    /// Perform an HTTP POST request.
    fn post<B, T>(
        &self,
        url: Url,
        body: &B,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
    where
        B: Serialize + Send + Sync,
        T: DeserializeOwned + Send + Sync;

    /// Perform a Redfish session creation POST request.
    fn post_session<B, T>(
        &self,
        url: Url,
        body: &B,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<SessionCreateResponse<T>, Self::Error>> + Send
    where
        B: Serialize + Send + Sync,
        T: DeserializeOwned + Send + Sync;

    /// Performs an UpdateService multipart upload with credentials and headers.
    ///
    /// The request carries `UpdateParameters`, `UpdateFile`, and optional OEM
    /// multipart parts.
    fn post_multipart_update<U, V, T>(
        &self,
        url: Url,
        request: MultipartUpdateRequest<'_, U, V>,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
    where
        U: UploadReader,
        T: DeserializeOwned + Send + Sync,
        V: Serialize + Send + Sync;

    /// Performs a deprecated `UpdateService` raw `HttpPushUri` upload with
    /// credentials and headers.
    ///
    /// This supports the deprecated `HttpPushUri` update path that exists in
    /// the Redfish spec. Prefer multipart update for BMCs that support
    /// `MultipartHttpPushUri`.
    #[cfg(feature = "update-service-deprecated")]
    fn post_http_push_uri_update<U, T>(
        &self,
        url: Url,
        request: HttpPushUriUpdateRequest<U>,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
    where
        U: UploadReader,
        T: DeserializeOwned + Send + Sync;

    /// Perform an HTTP PATCH request.
    fn patch<B, T>(
        &self,
        url: Url,
        etag: ODataETag,
        body: &B,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
    where
        B: Serialize + Send + Sync,
        T: DeserializeOwned + Send + Sync;

    /// Perform an HTTP DELETE request.
    fn delete<T>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<ModificationResponse<T>, Self::Error>> + Send
    where
        T: DeserializeOwned + Send + Sync;

    /// Open an SSE stream
    fn sse<T: Sized + for<'de> Deserialize<'de> + Send>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
    ) -> impl Future<Output = Result<BoxTryStream<T, Self::Error>, Self::Error>> + Send;

    /// Open an SSE stream whose events carry their ids, resuming after
    /// `last_event_id` when one is given.
    ///
    /// The default forwards to [`HttpClient::sse`]: its events carry no id
    /// and nothing is sent to resume from. A client that surfaces SSE ids
    /// overrides it.
    fn sse_events<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        url: Url,
        credentials: &BmcCredentials,
        custom_headers: &HeaderMap,
        last_event_id: Option<&str>,
    ) -> impl Future<Output = Result<BoxTryStream<StreamEvent<T>, Self::Error>, Self::Error>> + Send
    where
        Self::Error: 'static,
    {
        let _ = last_event_id;
        async move {
            self.sse::<T>(url, credentials, custom_headers)
                .await
                .map(without_event_ids)
        }
    }
}

/// HTTP-based BMC implementation that wraps an [`HttpClient`].
///
/// This struct combines an HTTP client with BMC endpoint information and credentials
/// to provide a complete Redfish client implementation. It implements the [`Bmc`] trait
/// to provide standardized access to Redfish services.
///
/// For Redfish URI-reference fields, such as action targets and update or
/// stream URIs, this implementation resolves the URI reference against the
/// configured BMC endpoint. It sends configured credentials and custom headers
/// only when the resolved URL has the same parsed origin as the BMC endpoint.
/// Cross-origin values are rejected before transport so callers can inspect and
/// handle those targets explicitly.
///
/// # Type Parameters
///
/// * `C` - The HTTP client implementation to use
pub struct HttpBmc<C: HttpClient> {
    client: C,
    redfish_endpoint: RedfishEndpoint,
    credentials: RwLock<Arc<BmcCredentials>>,
    cache: RwLock<TypeErasedCarCache<Url>>,
    custom_headers: HeaderMap,

    // Response bodies and ETags are enabled or disabled together because a
    // 304 Not Modified response contains no replacement body.
    cache_enabled: bool,
}

impl<C: HttpClient> HttpBmc<C>
where
    C::Error: CacheableError,
{
    /// Create a new HTTP-based BMC client with ETag-based caching.
    ///
    /// # Arguments
    ///
    /// * `client` - The HTTP client implementation to use for requests
    /// * `redfish_endpoint` - The base URL of the Redfish service (e.g., `https://192.168.1.100`)
    /// * `credentials` - Authentication credentials for the BMC
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nv_redfish_bmc_http::HttpBmc;
    /// use nv_redfish_bmc_http::CacheSettings;
    /// use nv_redfish_bmc_http::BmcCredentials;
    /// use nv_redfish_bmc_http::reqwest::Client;
    /// use url::Url;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let credentials = BmcCredentials::username_password("admin".to_string(), Some("password".to_string()));
    /// let http_client = Client::new()?;
    /// let endpoint = Url::parse("https://192.168.1.100")?;
    ///
    /// let bmc = HttpBmc::new(http_client, endpoint, credentials, CacheSettings::default());
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        client: C,
        redfish_endpoint: Url,
        credentials: BmcCredentials,
        cache_settings: CacheSettings,
    ) -> Self {
        Self::with_custom_headers(
            client,
            redfish_endpoint,
            credentials,
            cache_settings,
            HeaderMap::new(),
        )
    }

    /// Create a new HTTP-based BMC client with custom headers and ETag-based caching.
    ///
    /// This is an alternative constructor that allows specifying custom HTTP headers
    /// that will be included in all requests. Use this when you need vendor-specific
    /// headers, custom authentication tokens, or other HTTP headers required by the
    /// Redfish service at construction time.
    ///
    /// For most use cases, prefer [`HttpBmc::new`] which creates a client without
    /// custom headers.
    ///
    /// # Arguments
    ///
    /// * `client` - The HTTP client implementation to use for requests
    /// * `redfish_endpoint` - The base URL of the Redfish service (e.g., `https://192.168.1.100`)
    /// * `credentials` - Authentication credentials for the BMC
    /// * `cache_settings` - Cache configuration for response caching
    /// * `custom_headers` - Custom HTTP headers to include in all requests
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use nv_redfish_bmc_http::HttpBmc;
    /// use nv_redfish_bmc_http::CacheSettings;
    /// use nv_redfish_bmc_http::BmcCredentials;
    /// use nv_redfish_bmc_http::reqwest::Client;
    /// use url::Url;
    /// use http::HeaderMap;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let credentials = BmcCredentials::username_password("admin".to_string(), Some("password".to_string()));
    /// let http_client = Client::new()?;
    /// let endpoint = Url::parse("https://192.168.1.100")?;
    ///
    /// // Create custom headers
    /// let mut headers = HeaderMap::new();
    /// headers.insert("X-Auth-Token", "custom-token-value".parse()?);
    /// headers.insert("X-Vendor-Header", "vendor-specific-value".parse()?);
    ///
    /// // Create BMC client with custom headers
    /// let bmc = HttpBmc::with_custom_headers(
    ///     http_client,
    ///     endpoint,
    ///     credentials,
    ///     CacheSettings::default(),
    ///     headers,
    /// );
    ///
    /// // All requests will include the custom headers
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_custom_headers(
        client: C,
        redfish_endpoint: Url,
        credentials: BmcCredentials,
        cache_settings: CacheSettings,
        custom_headers: HeaderMap,
    ) -> Self {
        Self {
            client,
            redfish_endpoint: RedfishEndpoint::from(redfish_endpoint),
            credentials: RwLock::new(Arc::new(credentials)),
            cache: RwLock::new(TypeErasedCarCache::new(cache_settings.capacity)),
            custom_headers,
            cache_enabled: cache_settings.capacity > 0,
        }
    }

    /// Replace the credentials used for subsequent requests.
    ///
    /// Existing cache and ETag state is preserved.
    ///
    /// # Panics
    ///
    /// Panics if the internal credentials lock is poisoned. This should not
    /// occur in normal operation.
    #[allow(clippy::panic)] // See panics section.
    pub fn set_credentials(&self, credentials: BmcCredentials) {
        *self.credentials.write().expect("poisoned") = Arc::new(credentials);
    }
}

/// A tagged type representing a Redfish endpoint URL.
///
/// Provides convenient conversion methods to build endpoint URLs from `ODataId` paths.
#[derive(Debug, Clone)]
pub struct RedfishEndpoint {
    base_url: Url,
}

/// Service-provided URI reference that must be resolved as a URI reference.
///
/// Constructing this marker is the internal opt-in for Redfish fields whose
/// schemas allow URI references. Keep ordinary `ODataId` paths on
/// [`RedfishEndpoint::with_path`] so they remain scoped to the configured BMC
/// endpoint.
#[derive(Clone, Copy)]
struct UriReference<'a>(&'a str);

/// Error for a service URI reference rejected before transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedUriReferenceError {
    /// Reason the service URI reference was rejected.
    pub reason: String,
}

impl StdError for RejectedUriReferenceError {}

impl fmt::Display for RejectedUriReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(f)
    }
}

impl RedfishEndpoint {
    /// Create a new `RedfishEndpoint` from a base URL
    #[must_use]
    pub const fn new(base_url: Url) -> Self {
        Self { base_url }
    }

    /// Convert a path to a full Redfish endpoint URL
    #[must_use]
    pub fn with_path(&self, path: &str) -> Url {
        let mut url = self.base_url.clone();
        url.set_path(path);
        url
    }

    /// Convert an OData identifier, including its optional query, to an endpoint URL.
    ///
    /// `ODataId` is opaque and can contain a query, particularly when it comes
    /// from a task or session `Location` header. Passing the whole identifier to
    /// `Url::set_path` would encode `?` as path data, so split the two URL
    /// components before applying them.
    fn with_odata_id(&self, id: &ODataId) -> Url {
        let id = id.to_string();
        let (path, query) = id
            .split_once('?')
            .map_or((id.as_str(), None), |(path, query)| (path, Some(query)));

        let mut url = self.with_path(path);
        url.set_query(query);
        url
    }

    /// Convert an OData identifier and append query parameters.
    ///
    /// Existing parameters can carry continuation or monitor tokens and must
    /// survive when callers add `$expand` or `$filter` parameters.
    fn with_odata_id_and_query(&self, id: &ODataId, query: &str) -> Url {
        let mut url = self.with_odata_id(id);

        match url.query() {
            Some(existing) if !existing.is_empty() => {
                let combined_query = format!("{existing}&{query}");
                url.set_query(Some(&combined_query));
            }
            _ => url.set_query(Some(query)),
        }

        url
    }

    /// Resolve a URI reference and verify that it stays on the BMC origin.
    ///
    /// Callers must explicitly wrap service-provided values in
    /// [`UriReference`] before using this method. Use it only for schema fields
    /// that allow URI references, such as action targets,
    /// `MultipartHttpPushUri`, `HttpPushUri`, and event stream URIs. Same-origin
    /// is checked with parsed URL origins: scheme, host, and effective port. It
    /// is not a string prefix check, so prefix lookalikes such as
    /// `https://bmc.example.evil/...` are rejected.
    ///
    /// With base URL `https://bmc.example`:
    /// - `https://bmc.example/redfish/v1/Actions/Reset` is accepted.
    /// - `//bmc.example/redfish/v1/EventService/SSE` resolves to
    ///   `https://bmc.example/redfish/v1/EventService/SSE`.
    /// - `/redfish/v1/Systems/1/Actions/ComputerSystem.Reset` resolves to
    ///   `https://bmc.example/redfish/v1/Systems/1/Actions/ComputerSystem.Reset`.
    /// - `redfish/v1/UpdateService/upload` resolves relative to the base URL.
    /// - `https://bmc.example.evil/redfish/v1/Actions/Reset` is rejected.
    /// - `//host:99999/path` is rejected because the authority is malformed.
    ///
    /// Relative values without a leading slash use standard URI-reference
    /// resolution. If the configured base URL includes a path component, that
    /// can differ from direct path replacement. Values rejected by
    /// URI-reference resolution are rejected before transport.
    fn with_same_origin_uri_reference(
        &self,
        uri: UriReference<'_>,
    ) -> Result<Url, RejectedUriReferenceError> {
        let UriReference(uri) = uri;

        let resolved = self
            .base_url
            .join(uri)
            .map_err(|source| RejectedUriReferenceError {
                reason: format!(
                    "service URI reference `{}` could not be resolved against \
                     BMC endpoint `{}`: {source}",
                    uri, self.base_url
                ),
            })?;

        if resolved.origin() != self.base_url.origin() {
            return Err(RejectedUriReferenceError {
                reason: format!(
                    "service URI reference `{}` resolved to `{resolved}`, \
                     which is not same-origin with BMC endpoint `{}`",
                    uri, self.base_url
                ),
            });
        }

        Ok(resolved)
    }

    /// Convert a path to a full Redfish endpoint URL with query parameters
    #[must_use]
    pub fn with_path_and_query(&self, path: &str, query: &str) -> Url {
        let mut url = self.with_path(path);
        url.set_query(Some(query));
        url
    }
}

/// `CacheSettings` for internal BMC cache with etags
#[derive(Clone, Copy)]
pub struct CacheSettings {
    capacity: usize,
}

impl Default for CacheSettings {
    fn default() -> Self {
        Self { capacity: 100 }
    }
}

impl CacheSettings {
    /// Define capacity of the cache measured in number of items.
    ///
    /// A capacity of 0 disables caching.
    #[must_use]
    pub const fn with_capacity(capacity: usize) -> Self {
        Self { capacity }
    }
}

impl From<Url> for RedfishEndpoint {
    fn from(url: Url) -> Self {
        Self::new(url)
    }
}

impl From<&RedfishEndpoint> for Url {
    fn from(endpoint: &RedfishEndpoint) -> Self {
        endpoint.base_url.clone()
    }
}

/// Trait for errors that can indicate whether they represent a cached response
/// and provide a way to create cache-related errors.
pub trait CacheableError {
    /// Returns true if this error indicates the resource should be served from cache.
    /// Typically true for HTTP 304 Not Modified responses.
    fn is_cached(&self) -> bool;

    /// Create an error for when cached data is requested but not available.
    fn cache_miss() -> Self;

    /// Cache error
    fn cache_error(reason: String) -> Self;
}

/// Trait for errors that can represent request failures raised before transport.
pub trait RequestError {
    /// Create an error from a rejected service URI reference.
    fn rejected_uri_reference(error: RejectedUriReferenceError) -> Self;
}

impl<C: HttpClient> HttpBmc<C>
where
    C::Error: CacheableError + RequestError + StdError + Send + Sync,
{
    #[allow(clippy::panic)] // See set_credentials Panic doc.
    fn read_credentials(&self) -> Arc<BmcCredentials> {
        self.credentials
            .read()
            .map(|credentials| Arc::clone(&credentials))
            .expect("lock poisoned")
    }

    /// Perform a GET request with `ETag` caching support
    ///
    /// This handles:
    /// - Retrieving cached `ETag` before request
    /// - Sending conditional GET with If-None-Match
    /// - Handling 304 Not Modified responses from cache
    /// - Updating cache and `ETag` storage on success
    #[allow(clippy::significant_drop_tightening)]
    async fn get_with_cache<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        endpoint_url: Url,
    ) -> Result<Arc<T>, C::Error> {
        let cache_key = endpoint_url.clone();

        // The `etag` is always `None` when caching is disabled. Check the flag here so we can save
        // a read lock acquisition and guarantee that disabled caching never sends If-None-Match,
        // which could produce a 304 response without a cached body.
        // We need to get cached body before the request unless it can be evicted during GET
        // if cache reach the capacity.

        let cached = if self.cache_enabled {
            let mut cache = self
                .cache
                .write()
                .map_err(|e| C::Error::cache_error(e.to_string()))?;
            cache.get_typed::<Arc<T>>(&cache_key).cloned()
        } else {
            None
        };

        let etag = cached
            .as_ref()
            .and_then(|c| c.etag())
            .map(ToOwned::to_owned);
        let credentials = self.read_credentials();

        // Perform GET request
        match self
            .client
            .get::<T>(
                endpoint_url,
                credentials.as_ref(),
                etag,
                &self.custom_headers,
            )
            .await
        {
            Ok(response) if !self.cache_enabled => {
                // With capacity zero, `put_typed` stores no representation and always returns
                // `None`, and we can return early with the response entity.
                Ok(Arc::new(response))
            }
            Ok(response) => {
                let entity = Arc::new(response);
                // Update cache if entity has etag
                if entity.etag().is_some() {
                    let mut cache = self
                        .cache
                        .write()
                        .map_err(|e| C::Error::cache_error(e.to_string()))?;

                    cache.put_typed(cache_key.clone(), entity.clone());
                }
                Ok(entity)
            }
            Err(e) => {
                // Handle 304 Not Modified - return from cache
                if e.is_cached() {
                    cached.ok_or_else(C::Error::cache_miss)
                } else {
                    Err(e)
                }
            }
        }
    }
}

impl<C: HttpClient> Bmc for HttpBmc<C>
where
    C::Error: CacheableError + RequestError + StdError + Send + Sync,
{
    type Error = C::Error;

    async fn get<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        let endpoint_url = self.redfish_endpoint.with_odata_id(id);
        self.get_with_cache(endpoint_url).await
    }

    async fn expand<T: Expandable + 'static>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
    ) -> Result<Arc<T>, Self::Error> {
        let endpoint_url = self
            .redfish_endpoint
            .with_odata_id_and_query(id, &query.to_query_string());

        self.get_with_cache(endpoint_url).await
    }

    async fn create<V: Sync + Send + Serialize, R: Sync + Send + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        v: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let endpoint_url = self.redfish_endpoint.with_odata_id(id);
        let credentials = self.read_credentials();
        self.client
            .post(endpoint_url, v, credentials.as_ref(), &self.custom_headers)
            .await
    }

    async fn create_session<
        V: Sync + Send + Serialize,
        R: Sync + Send + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        v: &V,
    ) -> Result<SessionCreateResponse<R>, Self::Error> {
        let endpoint_url = self.redfish_endpoint.with_odata_id(id);
        self.client
            .post_session(endpoint_url, v, &self.custom_headers)
            .await
    }

    async fn update<V: Sync + Send + Serialize, R: Sync + Send + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        etag: Option<&ODataETag>,
        v: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let endpoint_url = self.redfish_endpoint.with_odata_id(id);
        let etag = etag
            .cloned()
            .unwrap_or_else(|| ODataETag::from(String::from("*")));
        let credentials = self.read_credentials();
        self.client
            .patch(
                endpoint_url,
                etag,
                v,
                credentials.as_ref(),
                &self.custom_headers,
            )
            .await
    }

    async fn delete<T: Sync + Send + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
    ) -> Result<ModificationResponse<T>, Self::Error> {
        let endpoint_url = self.redfish_endpoint.with_odata_id(id);
        let credentials = self.read_credentials();
        self.client
            .delete(endpoint_url, credentials.as_ref(), &self.custom_headers)
            .await
    }

    async fn action<T: Send + Sync + Serialize, R: Send + Sync + for<'de> Deserialize<'de>>(
        &self,
        action: &Action<T, R>,
        params: &T,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let endpoint_url = self
            .redfish_endpoint
            .with_same_origin_uri_reference(UriReference(action.target.as_str()))
            .map_err(C::Error::rejected_uri_reference)?;

        let credentials = self.read_credentials();
        self.client
            .post(
                endpoint_url,
                params,
                credentials.as_ref(),
                &self.custom_headers,
            )
            .await
    }

    async fn multipart_update<U, V, R>(
        &self,
        uri: &str,
        request: MultipartUpdateRequest<'_, U, V>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
        V: Send + Sync + Serialize,
    {
        let endpoint_url = self
            .redfish_endpoint
            .with_same_origin_uri_reference(UriReference(uri))
            .map_err(C::Error::rejected_uri_reference)?;

        let credentials = self.read_credentials();

        self.client
            .post_multipart_update(
                endpoint_url,
                request,
                credentials.as_ref(),
                &self.custom_headers,
            )
            .await
    }

    #[cfg(feature = "update-service-deprecated")]
    async fn http_push_uri_update<U, R>(
        &self,
        uri: &str,
        request: HttpPushUriUpdateRequest<U>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
    {
        let endpoint_url = self
            .redfish_endpoint
            .with_same_origin_uri_reference(UriReference(uri))
            .map_err(C::Error::rejected_uri_reference)?;

        let credentials = self.read_credentials();

        self.client
            .post_http_push_uri_update(
                endpoint_url,
                request,
                credentials.as_ref(),
                &self.custom_headers,
            )
            .await
    }

    async fn filter<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
    ) -> Result<Arc<T>, Self::Error> {
        let endpoint_url = self
            .redfish_endpoint
            .with_odata_id_and_query(id, &query.to_query_string());

        self.get_with_cache(endpoint_url).await
    }

    async fn stream<T: Send + Sized + for<'de> Deserialize<'de>>(
        &self,
        uri: &str,
    ) -> Result<BoxTryStream<T, Self::Error>, Self::Error> {
        let endpoint_url = self
            .redfish_endpoint
            .with_same_origin_uri_reference(UriReference(uri))
            .map_err(C::Error::rejected_uri_reference)?;

        let credentials = self.read_credentials();
        self.client
            .sse(endpoint_url, credentials.as_ref(), &self.custom_headers)
            .await
    }

    async fn stream_events<T: Send + Sized + for<'de> Deserialize<'de> + 'static>(
        &self,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> Result<BoxTryStream<StreamEvent<T>, Self::Error>, Self::Error>
    where
        C::Error: 'static,
    {
        let endpoint_url = self
            .redfish_endpoint
            .with_same_origin_uri_reference(UriReference(uri))
            .map_err(C::Error::rejected_uri_reference)?;

        let credentials = self.read_credentials();
        self.client
            .sse_events(
                endpoint_url,
                credentials.as_ref(),
                &self.custom_headers,
                last_event_id,
            )
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn same_origin_uri_reference_matches_documented_examples() -> Result<(), Box<dyn Error>> {
        let endpoint = RedfishEndpoint::new(Url::parse("https://bmc.example")?);

        let cases = [
            (
                "https://bmc.example/redfish/v1/Actions/Reset",
                "https://bmc.example/redfish/v1/Actions/Reset",
            ),
            (
                "//bmc.example/redfish/v1/EventService/SSE",
                "https://bmc.example/redfish/v1/EventService/SSE",
            ),
            (
                "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                "https://bmc.example/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
            ),
            (
                "redfish/v1/UpdateService/upload",
                "https://bmc.example/redfish/v1/UpdateService/upload",
            ),
        ];

        for (uri, expected) in cases {
            assert_eq!(
                endpoint
                    .with_same_origin_uri_reference(UriReference(uri))?
                    .as_str(),
                expected,
                "{uri}"
            );
        }

        Ok(())
    }

    #[test]
    fn odata_id_query_is_preserved_as_url_query() -> Result<(), Box<dyn Error>> {
        let endpoint = RedfishEndpoint::new(Url::parse("https://bmc.example")?);
        let id =
            ODataId::from("/redfish/v1/TaskService/Tasks/42?token=a%2Fb&state=ready".to_string());

        let resolved = endpoint.with_odata_id(&id);

        assert_eq!(resolved.path(), "/redfish/v1/TaskService/Tasks/42");
        assert_eq!(resolved.query(), Some("token=a%2Fb&state=ready"));

        Ok(())
    }

    #[test]
    fn odata_id_query_is_combined_with_request_query() -> Result<(), Box<dyn Error>> {
        let endpoint = RedfishEndpoint::new(Url::parse("https://bmc.example")?);
        let id = ODataId::from("/redfish/v1/Systems?$skiptoken=abc".to_string());

        let resolved = endpoint.with_odata_id_and_query(&id, "$filter=value%20gt%2010");

        assert_eq!(resolved.path(), "/redfish/v1/Systems");

        assert_eq!(
            resolved.query(),
            Some("$skiptoken=abc&$filter=value%20gt%2010")
        );

        Ok(())
    }

    #[test]
    fn uri_reference_relative_path_follows_base_path() -> Result<(), Box<dyn Error>> {
        let endpoint = RedfishEndpoint::new(Url::parse("https://bmc.example/proxy/")?);

        let resolved = endpoint
            .with_same_origin_uri_reference(UriReference("redfish/v1/UpdateService/upload"))?;

        assert_eq!(
            resolved.as_str(),
            "https://bmc.example/proxy/redfish/v1/UpdateService/upload"
        );

        Ok(())
    }

    #[test]
    fn rejects_prefix_lookalike_uri_reference() -> Result<(), Box<dyn Error>> {
        let endpoint = RedfishEndpoint::new(Url::parse("https://bmc.example")?);

        let result = endpoint.with_same_origin_uri_reference(UriReference(
            "https://bmc.example.evil/redfish/v1/Actions/Reset",
        ));

        let error = result.expect_err("expected cross-origin URI reference error");

        assert!(error.reason.contains("not same-origin"));

        Ok(())
    }

    #[test]
    fn rejects_malformed_authority_uri_reference() -> Result<(), Box<dyn Error>> {
        let endpoint = RedfishEndpoint::new(Url::parse("https://bmc.example")?);

        for uri in ["//[:::1]/path", "//host:99999/path"] {
            let result = endpoint.with_same_origin_uri_reference(UriReference(uri));

            let error = result.expect_err("expected malformed URI reference error");

            assert!(error.reason.contains("could not be resolved"));
        }

        Ok(())
    }
}
