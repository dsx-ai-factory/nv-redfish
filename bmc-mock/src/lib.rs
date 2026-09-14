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

pub mod expect;

#[doc(inline)]
pub use expect::Expect;
pub use expect::ExpectedRequest;

use std::collections::VecDeque;
use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use nv_redfish_core::action::ActionTarget;
use nv_redfish_core::query::ExpandQuery;
use nv_redfish_core::ActionError;
use nv_redfish_core::Bmc as NvRedfishBmc;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::HttpPushUriUpdateRequest;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::MultipartUpdateRequest;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::SessionCreateResponse;
use nv_redfish_core::UploadReader;
use serde::Serialize;
use serde_json::from_value;
use serde_json::to_value;
use serde_json::Error as JsonError;

#[derive(Debug)]
pub enum Error {
    NotSupported,
    ErrorResponse(Box<dyn StdError + Send + Sync>),
    MutexLock(String),
    NothingIsExpected,
    BadResponseJson(JsonError),
    UnexpectedGet(ODataId, ExpectedRequest),
    UnexpectedExpand(ODataId, ExpectedRequest),
    UnexpectedUpdate(ODataId, String, ExpectedRequest),
    UnexpectedCreate(ODataId, String, ExpectedRequest),
    UnexpectedCreateSession(ODataId, String, ExpectedRequest),
    UnexpectedDelete(ODataId, ExpectedRequest),
    UnexpectedAction(ActionTarget, String, ExpectedRequest),
    UnexpectedMultipartUpdate(String, String, String, ExpectedRequest),
    #[cfg(feature = "update-service-deprecated")]
    UnexpectedHttpPushUriUpdate(String, ExpectedRequest),
    UnexpectedStream(String, ExpectedRequest),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::ErrorResponse(err) => write!(f, "response: {err}"),
            Self::NotSupported => write!(f, "not supported"),
            Self::MutexLock(err) => write!(f, "lock error: {err}"),
            Self::NothingIsExpected => {
                write!(f, "nothing is expected to happen but something happened")
            }
            Self::BadResponseJson(err) => write!(f, "bad json response: {err}"),
            Self::UnexpectedGet(id, expected) => {
                write!(f, "unexpected get: {id}; expected: {expected:?}")
            }
            Self::UnexpectedExpand(id, expected) => {
                write!(f, "unexpected expand: {id}; expected: {expected:?}")
            }
            Self::UnexpectedUpdate(id, json, expected) => {
                write!(
                    f,
                    "unexpected update: {id}; json: {json} expected: {expected:?}"
                )
            }
            Self::UnexpectedCreate(id, json, expected) => {
                write!(
                    f,
                    "unexpected create: {id}; json: {json} expected: {expected:?}"
                )
            }
            Self::UnexpectedCreateSession(id, json, expected) => {
                write!(
                    f,
                    "unexpected session create: {id}; json: {json} expected: {expected:?}"
                )
            }
            Self::UnexpectedDelete(id, expected) => {
                write!(f, "unexpected delete: {id}; expected: {expected:?}")
            }
            Self::UnexpectedAction(id, json, expected) => {
                write!(
                    f,
                    "unexpected action: {id}; json: {json} expected: {expected:?}"
                )
            }
            Self::UnexpectedMultipartUpdate(uri, json, file, expected) => {
                write!(
                    f,
                    "unexpected multipart update: {uri}; json: {json}; file: {file}; expected: {expected:?}"
                )
            }
            #[cfg(feature = "update-service-deprecated")]
            Self::UnexpectedHttpPushUriUpdate(uri, expected) => {
                write!(
                    f,
                    "unexpected HttpPushUri update: {uri}; expected: {expected:?}"
                )
            }
            Self::UnexpectedStream(uri, expected) => {
                write!(f, "unexpected stream: {uri}; expected: {expected:?}")
            }
        }
    }
}

impl StdError for Error {}

impl Error {
    pub fn mutex_lock<T>(err: PoisonError<T>) -> Self {
        Self::MutexLock(err.to_string())
    }
}

pub struct Bmc<E> {
    expect: Mutex<VecDeque<Expect<E>>>,
}

impl<E> Default for Bmc<E> {
    fn default() -> Self {
        Self {
            expect: Mutex::default(),
        }
    }
}

impl<E> Bmc<E> {
    pub fn expect(&self, exp: Expect<E>) {
        let expect: &mut VecDeque<Expect<E>> = &mut self.expect.lock().expect("not poisoned");
        expect.push_back(exp);
    }

    pub fn debug_expect(&self) {
        let expect: &VecDeque<Expect<E>> = &self.expect.lock().expect("not poisoned");
        println!("Expectations (total: {})", expect.len());
        for v in expect {
            println!("{:#?}", v.request);
        }
    }
}

impl<E> NvRedfishBmc for Bmc<E>
where
    E: StdError + Send + Sync + 'static,
{
    type Error = Error;

    async fn expand<T>(&self, in_id: &ODataId, _query: ExpandQuery) -> Result<Arc<T>, Error>
    where
        T: Expandable,
    {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;
        match expect {
            Expect {
                request: ExpectedRequest::Expand { id },
                response,
            } if id == *in_id => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: T = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(Arc::new(result))
            }
            _ => Err(Error::UnexpectedExpand(in_id.clone(), expect.request)),
        }
    }

    async fn get<T: EntityTypeRef + for<'de> serde::Deserialize<'de>>(
        &self,
        in_id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;
        match expect {
            Expect {
                request: ExpectedRequest::Get { id },
                response,
            } if id == *in_id => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: T = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(Arc::new(result))
            }
            _ => Err(Error::UnexpectedGet(in_id.clone(), expect.request)),
        }
    }

    async fn update<
        V: Sync + Send + Serialize,
        R: Sync + Send + Sized + for<'de> serde::Deserialize<'de>,
    >(
        &self,
        in_id: &ODataId,
        _etag: Option<&ODataETag>,
        update: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;

        let in_request = to_value(update).expect("json serializable");

        match expect {
            Expect {
                request: ExpectedRequest::Update { id, request },
                response,
            } if id == *in_id && request == in_request => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: R = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(ModificationResponse::Entity(result))
            }
            Expect {
                request: ExpectedRequest::UpdateTask { id, request, task },
                ..
            } if id == *in_id && request == in_request => Ok(ModificationResponse::Task(task)),
            Expect {
                request: ExpectedRequest::UpdateEmpty { id, request },
                ..
            } if id == *in_id && request == in_request => Ok(ModificationResponse::Empty),
            _ => Err(Error::UnexpectedUpdate(
                in_id.clone(),
                in_request.to_string(),
                expect.request,
            )),
        }
    }

    async fn create<
        V: Sync + Send + Serialize,
        R: Sync + Send + Sized + for<'de> serde::Deserialize<'de>,
    >(
        &self,
        in_id: &ODataId,
        create: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;

        let in_request = to_value(create).expect("json serializable");

        match expect {
            Expect {
                request: ExpectedRequest::Create { id, request },
                response,
            } if id == *in_id && request == in_request => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: R = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(ModificationResponse::Entity(result))
            }
            Expect {
                request: ExpectedRequest::CreateTask { id, request, task },
                ..
            } if id == *in_id && request == in_request => Ok(ModificationResponse::Task(task)),
            Expect {
                request: ExpectedRequest::CreateEmpty { id, request },
                ..
            } if id == *in_id && request == in_request => Ok(ModificationResponse::Empty),
            _ => Err(Error::UnexpectedCreate(
                in_id.clone(),
                in_request.to_string(),
                expect.request,
            )),
        }
    }

    async fn create_session<
        V: Sync + Send + Serialize,
        R: Sync + Send + Sized + for<'de> serde::Deserialize<'de>,
    >(
        &self,
        in_id: &ODataId,
        create: &V,
    ) -> Result<SessionCreateResponse<R>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;
        let in_request = to_value(create).expect("json serializable");
        match expect {
            Expect {
                request:
                    ExpectedRequest::CreateSession {
                        id,
                        request,
                        auth_token,
                        location,
                    },
                response,
            } if id == *in_id && request == in_request => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let entity: R = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(SessionCreateResponse {
                    entity,
                    auth_token,
                    location,
                })
            }
            _ => Err(Error::UnexpectedCreateSession(
                in_id.clone(),
                in_request.to_string(),
                expect.request,
            )),
        }
    }

    async fn delete<R: EntityTypeRef + for<'de> serde::Deserialize<'de>>(
        &self,
        in_id: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;
        match expect {
            Expect {
                request: ExpectedRequest::Delete { id },
                ..
            } if id == *in_id => Ok(ModificationResponse::Empty),
            Expect {
                request: ExpectedRequest::DeleteTask { id, task },
                ..
            } if id == *in_id => Ok(ModificationResponse::Task(task)),
            _ => Err(Error::UnexpectedDelete(in_id.clone(), expect.request)),
        }
    }

    async fn action<
        T: Send + Sync + serde::Serialize,
        R: Send + Sync + Sized + for<'de> serde::Deserialize<'de>,
    >(
        &self,
        action: &nv_redfish_core::Action<T, R>,
        params: &T,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;
        let in_request = to_value(params).expect("json serializable");
        match expect {
            Expect {
                request: ExpectedRequest::Action { target, request },
                response,
            } if target == action.target && request == in_request => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: R = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(ModificationResponse::Entity(result))
            }
            _ => Err(Error::UnexpectedAction(
                action.target.clone(),
                in_request.to_string(),
                expect.request,
            )),
        }
    }

    async fn multipart_update<U, V, R>(
        &self,
        in_uri: &str,
        update_request: MultipartUpdateRequest<'_, U, V>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
        V: Send + Sync + Serialize,
    {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;

        let MultipartUpdateRequest {
            update_parameters,
            update_stream,
            oem_parts,
            ..
        } = update_request;
        let in_request = to_value(update_parameters).expect("json serializable");
        let file_name = update_stream.name;
        let oem_parts = oem_parts
            .into_iter()
            .map(|part| part.name)
            .collect::<Vec<_>>();

        match expect {
            Expect {
                request:
                    ExpectedRequest::MultipartUpdate {
                        uri,
                        request,
                        file_name: expected_file_name,
                        oem_parts: expected_parts,
                    },
                response,
            } if uri == *in_uri
                && request == in_request
                && expected_file_name == file_name
                && expected_parts == oem_parts =>
            {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: R = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(ModificationResponse::Entity(result))
            }
            _ => Err(Error::UnexpectedMultipartUpdate(
                in_uri.to_string(),
                in_request.to_string(),
                file_name,
                expect.request,
            )),
        }
    }

    #[cfg(feature = "update-service-deprecated")]
    async fn http_push_uri_update<U, R>(
        &self,
        in_uri: &str,
        _update_request: HttpPushUriUpdateRequest<U>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;

        match expect {
            Expect {
                request: ExpectedRequest::HttpPushUriUpdate { uri },
                response,
            } if uri == *in_uri => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: R = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(ModificationResponse::Entity(result))
            }
            _ => Err(Error::UnexpectedHttpPushUriUpdate(
                in_uri.to_string(),
                expect.request,
            )),
        }
    }

    async fn filter<T: EntityTypeRef + for<'de> serde::Deserialize<'de>>(
        &self,
        _id: &ODataId,
        _query: nv_redfish_core::FilterQuery,
    ) -> Result<Arc<T>, Self::Error> {
        todo!("unimplemented")
    }

    async fn stream<T: Sized + for<'de> serde::Deserialize<'de> + Send + 'static>(
        &self,
        in_uri: &str,
    ) -> Result<nv_redfish_core::BoxTryStream<T, Self::Error>, Self::Error> {
        let expect = self
            .expect
            .lock()
            .map_err(Error::mutex_lock)?
            .pop_front()
            .ok_or(Error::NothingIsExpected)?;
        match expect {
            Expect {
                request: ExpectedRequest::Stream { uri },
                response,
            } if uri == *in_uri => {
                let response = response.map_err(|err| Error::ErrorResponse(Box::new(err)))?;
                let result: Vec<T> = from_value(response).map_err(Error::BadResponseJson)?;
                Ok(Box::pin(futures_util::stream::iter(
                    result.into_iter().map(Ok),
                )))
            }
            _ => Err(Error::UnexpectedStream(in_uri.to_string(), expect.request)),
        }
    }
}

impl ActionError for Error {
    fn not_supported() -> Self {
        Error::NotSupported
    }
}
