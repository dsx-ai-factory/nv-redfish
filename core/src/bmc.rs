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

//! Baseboard Management Controller (BMC) client abstraction
//!
//! This module defines the transport-agnostic [`Bmc`] trait — a minimal
//! interface for interacting with Redfish services. Implementors provide
//! asynchronous operations to retrieve and expand entities, create/update
//! resources, delete entities, and invoke actions.
//!
//! Key concepts:
//! - Entity identity: Every entity is identified by an `@odata.id` ([`crate::ODataId`]).
//! - Entity reference: Generated types implement [`crate::EntityTypeRef`], which
//!   exposes `id()` and optional `etag()` accessors.
//! - Arc-based sharing: Read operations return `Arc<T>` to enable cheap sharing
//!   and caching while keeping values immutable.
//! - Expansion: [`crate::Expandable`] entities can request inline expansion using
//!   [`crate::query::ExpandQuery`], matching Redfish DSP0266 semantics for `$expand`.
//! - Actions: Actions are described by [`crate::Action<T, R>`] and are invoked via
//!   the `action` method.
//!
//! Operation semantics:
//! - `get` fetches the entity at the given `@odata.id`.
//! - `expand` fetches the entity with the provided `$expand` query.
//! - `create` typically performs a POST to a collection identified by `id` and
//!   returns the server-provided representation (`R`).
//! - `update` typically performs a PATCH on an entity identified by `id` and
//!   returns the updated representation (`R`).
//! - `delete` removes the entity at `id`.
//! - `action` posts to an action endpoint (`Action.target`).
//!
//! Notes for implementors:
//! - The trait is `Send + Sync` and returns `Send` futures to support use in
//!   async runtimes and multithreaded contexts.
//! - Implementations may include client-side caching or conditional requests;
//!   these details are intentionally abstracted behind the trait.
//! - Errors should implement `std::error::Error` and be safely transferable
//!   across threads.

use serde::Deserialize;
use serde::Serialize;

use crate::query::ExpandQuery;
use crate::Action;
use crate::BoxTryStream;
use crate::EntityTypeRef;
use crate::Expandable;
use crate::FilterQuery;
#[cfg(feature = "update-service-deprecated")]
use crate::HttpPushUriUpdateRequest;
use crate::ModificationResponse;
use crate::ODataETag;
use crate::ODataId;
use crate::SessionCreateResponse;
use std::error::Error as StdError;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use futures_core::Stream;

use crate::MultipartUpdateRequest;
use crate::UploadReader;

/// BMC trait defines access to a Baseboard Management Controller using
/// the Redfish protocol.
pub trait Bmc: Send + Sync {
    /// BMC Error.
    type Error: StdError + Send + Sync;

    /// Expand any expandable object (navigation property or entity).
    ///
    /// `T` is structure that is used for return type.
    fn expand<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
    ) -> impl Future<Output = Result<Arc<T>, Self::Error>> + Send;

    /// Get data of the object (navigation property or entity).
    ///
    /// `T` is structure that is used for return type.
    fn get<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
    ) -> impl Future<Output = Result<Arc<T>, Self::Error>> + Send;

    /// Get and filters data of the object (navigation property or entity).
    ///
    /// `T` is structure that is used for return type.
    fn filter<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
    ) -> impl Future<Output = Result<Arc<T>, Self::Error>> + Send;

    /// Creates element of the collection.
    ///
    /// `V` is structure that is used for create.
    /// `R` is structure that is used for return type.
    fn create<V: Send + Sync + Serialize, R: Send + Sync + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> impl Future<Output = Result<ModificationResponse<R>, Self::Error>> + Send;

    /// Creates a Redfish session.
    ///
    /// Session creation is special in Redfish: the response body contains the
    /// session entity, `X-Auth-Token` contains the token used for subsequent
    /// requests, and `Location` contains the URI to delete when logging out.
    fn create_session<V: Send + Sync + Serialize, R: Send + Sync + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> impl Future<Output = Result<SessionCreateResponse<R>, Self::Error>> + Send;

    /// Update entity.
    ///
    /// `V` is structure that is used for update.
    /// `R` is structure that is used for return type (updated entity).
    fn update<V: Sync + Send + Serialize, R: Send + Sync + Sized + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        etag: Option<&ODataETag>,
        update: &V,
    ) -> impl Future<Output = Result<ModificationResponse<R>, Self::Error>> + Send;

    /// Delete entity.
    fn delete<R: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
    ) -> impl Future<Output = Result<ModificationResponse<R>, Self::Error>> + Send;

    /// Run action.
    ///
    /// Implementations should resolve the action `target` as a Redfish URI
    /// reference. Absolute targets are allowed by Redfish and BMC-relative
    /// targets should remain supported.
    ///
    /// Implementations may reject URI references that violate their outbound
    /// request policy before transport.
    ///
    /// `T` is structure that contains action parameters.
    /// `R` is structure with return type.
    fn action<T: Send + Sync + Serialize, R: Send + Sync + Sized + for<'de> Deserialize<'de>>(
        &self,
        action: &Action<T, R>,
        params: &T,
    ) -> impl Future<Output = Result<ModificationResponse<R>, Self::Error>> + Send;

    /// POST a Redfish `UpdateService` multipart upload using a named stream.
    ///
    /// `uri` is the service-provided `MultipartHttpPushUri` and should be
    /// resolved as a Redfish URI reference.
    ///
    /// Implementations may reject URI references that violate their outbound
    /// request policy before transport.
    fn multipart_update<U, V, R>(
        &self,
        uri: &str,
        request: MultipartUpdateRequest<'_, U, V>,
    ) -> impl Future<Output = Result<ModificationResponse<R>, Self::Error>> + Send
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
        V: Send + Sync + Serialize;

    /// POST a raw binary stream to a Redfish `UpdateService` `HttpPushUri`.
    ///
    /// `uri` is the service-provided `HttpPushUri` and should be resolved as a
    /// Redfish URI reference.
    ///
    /// Implementations may reject URI references that violate their outbound
    /// request policy before transport.
    #[cfg(feature = "update-service-deprecated")]
    fn http_push_uri_update<U, R>(
        &self,
        uri: &str,
        request: HttpPushUriUpdateRequest<U>,
    ) -> impl Future<Output = Result<ModificationResponse<R>, Self::Error>> + Send
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>;

    /// Stream data for the URI.
    ///
    /// `uri` should be resolved as a Redfish URI reference.
    ///
    /// Implementations may reject URI references that violate their outbound
    /// request policy before transport.
    ///
    /// `T` is structure that is used for the stream return type.
    fn stream<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
    ) -> impl Future<Output = Result<BoxTryStream<T, Self::Error>, Self::Error>> + Send;

    /// Stream server-sent events from the URI with the id each carries,
    /// resuming after `last_event_id`.
    ///
    /// Same URI rules as [`Bmc::stream`]. Every item is a [`StreamEvent`]:
    /// the event's `data` decoded into `T`, with the event id in effect.
    /// `last_event_id` is the id a consumer kept from an earlier stream of
    /// the same URI, sent as `Last-Event-ID` so a server that retains
    /// history replays the events after it; `None` starts at the server's
    /// live position.
    ///
    /// The default forwards to [`Bmc::stream`]: its events carry no id and
    /// nothing is sent to resume from, which cannot mislead a consumer, since
    /// it only ever holds an id this method gave it. A transport that
    /// surfaces SSE ids overrides it.
    fn stream_events<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> impl Future<Output = Result<BoxTryStream<StreamEvent<T>, Self::Error>, Self::Error>> + Send
    where
        Self::Error: 'static,
    {
        let _ = last_event_id;
        async move { self.stream::<T>(uri).await.map(without_event_ids) }
    }
}

/// One event a server-sent event stream delivered.
///
/// `data` is the event's payload, decoded into the type the stream was
/// opened with. `last_event_id` is the SSE `id` in effect when the event was
/// dispatched: a server sets it per event, it persists over later events
/// that carry none, and an `id` field with an empty value clears it, as the
/// SSE processing model prescribes. A consumer that reopens the stream hands
/// it back through [`Bmc::stream_events`] to resume after this event. A
/// server that never sets one, or a transport that does not surface ids,
/// leaves it `None` throughout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamEvent<T> {
    /// The id to resume after this event, if the server set one.
    pub last_event_id: Option<String>,
    /// The event's decoded payload.
    pub data: T,
}

/// `stream` as a stream of [`StreamEvent`]s that carry no id: the view a
/// transport that does not surface SSE ids gives through
/// [`Bmc::stream_events`].
#[must_use]
pub fn without_event_ids<T, E>(stream: BoxTryStream<T, E>) -> BoxTryStream<StreamEvent<T>, E>
where
    T: 'static,
    E: 'static,
{
    Box::pin(WithoutEventIds(stream))
}

struct WithoutEventIds<T, E>(BoxTryStream<T, E>);

impl<T, E> Stream for WithoutEventIds<T, E> {
    type Item = Result<StreamEvent<T>, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0.as_mut().poll_next(cx).map(|item| {
            item.map(|result| {
                result.map(|data| StreamEvent {
                    last_event_id: None,
                    data,
                })
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::task::Waker;
    use std::vec::IntoIter;

    use super::*;

    /// A stream over a fixed list of items.
    struct Items(IntoIter<Result<u8, &'static str>>);

    impl Stream for Items {
        type Item = Result<u8, &'static str>;

        fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
            Poll::Ready(self.0.next())
        }
    }

    #[test]
    fn without_ids_keeps_items_and_errors_and_attaches_no_id() {
        let items = Items(vec![Ok(1), Err("lost"), Ok(2)].into_iter());
        let mut events = without_event_ids(Box::pin(items));
        let mut cx = Context::from_waker(Waker::noop());
        let event = |data| {
            Poll::Ready(Some(Ok(StreamEvent {
                last_event_id: None,
                data,
            })))
        };

        assert_eq!(events.as_mut().poll_next(&mut cx), event(1));
        assert_eq!(
            events.as_mut().poll_next(&mut cx),
            Poll::Ready(Some(Err("lost")))
        );
        assert_eq!(events.as_mut().poll_next(&mut cx), event(2));
        assert_eq!(events.as_mut().poll_next(&mut cx), Poll::Ready(None));
    }
}
