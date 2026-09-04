// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

use std::num::NonZeroUsize;
use std::sync::Arc;

use crate::{BmcCredentials, CacheableError, HttpBmc, HttpClient};

use nv_redfish_core::query::ExpandQuery;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::HttpPushUriUpdateRequest;
use nv_redfish_core::{
    Action, Bmc, BoxTryStream, EntityTypeRef, Expandable, FilterQuery, ModificationResponse,
    MultipartUpdateRequest, ODataETag, ODataId, SessionCreateResponse, UploadReader,
};

use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use tokio::sync::SemaphorePermit;

/// Limits the number of concurrent operations entering an inner BMC.
///
/// Each wrapper owns independent capacity. Construct separate wrappers around
/// BMC endpoints that share an HTTP client to retain independent endpoint
/// limits while allowing the HTTP client to share its connection pool.
///
/// A permit covers the complete inner [`Bmc`] operation, including transport
/// retries. Stream operations release their permit after connection
/// establishment, so the returned stream does not consume capacity.
/// Capacity waits are asynchronous and served in arrival order — a released
/// permit goes to the longest waiter, so no operation starves under
/// sustained load. Canceling a waiting operation does not consume a permit.
///
/// # Examples
///
/// ```rust,no_run
/// # #[cfg(feature = "reqwest")]
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use std::num::NonZeroUsize;
///
/// use nv_redfish_bmc_http::reqwest::Client;
/// use nv_redfish_bmc_http::{BmcCredentials, CacheSettings, HttpBmc};
/// use url::Url;
///
/// let client = Client::new()?;
/// let endpoint = Url::parse("https://bmc.example")?;
/// let credentials = BmcCredentials::username_password("admin".to_owned(), None);
/// let bmc = HttpBmc::new(client, endpoint, credentials, CacheSettings::default());
/// let _limited_bmc = bmc.with_request_concurrency_limit(NonZeroUsize::MIN);
/// # Ok(())
/// # }
/// ```
pub struct ConcurrencyLimitedBmc<B> {
    inner: B,
    semaphore: Semaphore,
}

impl<B> ConcurrencyLimitedBmc<B> {
    pub(crate) const fn new(inner: B, limit: NonZeroUsize) -> Self {
        // `const_new` asserts `permits <= MAX_PERMITS`; clamping keeps every
        // `NonZeroUsize` valid, as it was with the previous semaphore.
        let permits = if limit.get() > Semaphore::MAX_PERMITS {
            Semaphore::MAX_PERMITS
        } else {
            limit.get()
        };
        Self {
            inner,
            semaphore: Semaphore::const_new(permits),
        }
    }
}

/// Waits for a permit. Acquisition is fair (FIFO) and only ever waits:
/// the semaphore is never closed. A free function borrowing only the
/// semaphore, so the future is `Send` without bounding the wrapped `B`.
async fn permit(semaphore: &Semaphore) -> SemaphorePermit<'_> {
    semaphore
        .acquire()
        .await
        .expect("the semaphore is never closed")
}

impl<C: HttpClient> HttpBmc<C>
where
    C::Error: CacheableError,
{
    /// Configures the maximum number of concurrent Redfish operations.
    ///
    /// The limit covers complete logical operations, including transport
    /// retries. Without this method, the BMC remains unlimited. Limits above
    /// `usize::MAX >> 3` are treated as that value.
    #[must_use]
    pub const fn with_request_concurrency_limit(
        self,
        limit: NonZeroUsize,
    ) -> ConcurrencyLimitedBmc<Self> {
        ConcurrencyLimitedBmc::new(self, limit)
    }
}

impl<C: HttpClient> ConcurrencyLimitedBmc<HttpBmc<C>>
where
    C::Error: CacheableError,
{
    /// Replaces the credentials used by the wrapped [`HttpBmc`].
    ///
    /// # Panics
    ///
    /// Panics if the wrapped BMC's credentials lock is poisoned.
    pub fn set_credentials(&self, credentials: BmcCredentials) {
        self.inner.set_credentials(credentials);
    }
}

impl<B: Bmc> Bmc for ConcurrencyLimitedBmc<B> {
    type Error = B::Error;

    async fn expand<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
    ) -> Result<Arc<T>, Self::Error> {
        let _permit = permit(&self.semaphore).await;
        self.inner.expand(id, query).await
    }

    async fn get<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        let _permit = permit(&self.semaphore).await;
        self.inner.get(id).await
    }

    async fn filter<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
    ) -> Result<Arc<T>, Self::Error> {
        let _permit = permit(&self.semaphore).await;
        self.inner.filter(id, query).await
    }

    async fn create<V, R>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        V: Send + Sync + Serialize,
        R: Send + Sync + for<'de> Deserialize<'de>,
    {
        let _permit = permit(&self.semaphore).await;
        self.inner.create(id, query).await
    }

    async fn create_session<V, R>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<SessionCreateResponse<R>, Self::Error>
    where
        V: Send + Sync + Serialize,
        R: Send + Sync + for<'de> Deserialize<'de>,
    {
        let _permit = permit(&self.semaphore).await;
        self.inner.create_session(id, query).await
    }

    async fn update<V, R>(
        &self,
        id: &ODataId,
        etag: Option<&ODataETag>,
        update: &V,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        V: Sync + Send + Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    {
        let _permit = permit(&self.semaphore).await;
        self.inner.update(id, etag, update).await
    }

    async fn delete<R>(&self, id: &ODataId) -> Result<ModificationResponse<R>, Self::Error>
    where
        R: EntityTypeRef + for<'de> Deserialize<'de>,
    {
        let _permit = permit(&self.semaphore).await;
        self.inner.delete(id).await
    }

    async fn action<T, R>(
        &self,
        action: &Action<T, R>,
        params: &T,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        T: Send + Sync + Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    {
        let _permit = permit(&self.semaphore).await;
        self.inner.action(action, params).await
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
        let _permit = permit(&self.semaphore).await;
        self.inner.multipart_update(uri, request).await
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
        let _permit = permit(&self.semaphore).await;
        self.inner.http_push_uri_update(uri, request).await
    }

    async fn stream<T>(&self, uri: &str) -> Result<BoxTryStream<T, Self::Error>, Self::Error>
    where
        T: Sized + for<'de> Deserialize<'de> + Send + 'static,
    {
        let _permit = permit(&self.semaphore).await;
        self.inner.stream(uri).await
    }
}
