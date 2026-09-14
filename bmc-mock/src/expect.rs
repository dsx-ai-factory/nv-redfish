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

//! Expectations for Bmc Mock.

use std::fmt::Display;

use nv_redfish_core::action::ActionTarget;
use nv_redfish_core::AsyncTask;
use nv_redfish_core::ODataId;

use serde_json::from_str;
use serde_json::Value as JsonValue;

pub type Response<E> = Result<JsonValue, E>;

/// Request expected by BMC.
#[derive(Debug)]
pub enum ExpectedRequest {
    /// Expected Get.
    Get { id: ODataId },

    /// Expected Expand.
    Expand { id: ODataId },

    /// Expected Update.
    Update { id: ODataId, request: JsonValue },

    /// Expected asynchronous update.
    UpdateTask {
        id: ODataId,
        request: JsonValue,
        task: AsyncTask,
    },

    /// Expected update with no response body.
    UpdateEmpty { id: ODataId, request: JsonValue },

    /// Expected Create.
    Create { id: ODataId, request: JsonValue },

    /// Expected asynchronous create.
    CreateTask {
        id: ODataId,
        request: JsonValue,
        task: AsyncTask,
    },

    /// Expected create with no response body.
    CreateEmpty { id: ODataId, request: JsonValue },

    /// Expected Redfish session creation.
    CreateSession {
        id: ODataId,
        request: JsonValue,
        auth_token: String,
        location: ODataId,
    },

    /// Expected ActionTarget
    Action {
        target: ActionTarget,
        request: JsonValue,
    },

    /// Expected multipart update.
    MultipartUpdate {
        uri: String,
        request: JsonValue,
        file_name: String,
        oem_parts: Vec<String>,
    },

    /// Expected raw HttpPushUri update.
    #[cfg(feature = "update-service-deprecated")]
    HttpPushUriUpdate { uri: String },

    /// Expected Delete.
    Delete { id: ODataId },

    /// Expected asynchronous delete.
    DeleteTask { id: ODataId, task: AsyncTask },

    /// Expected Stream.
    Stream { uri: String },
}

/// Expectation for the tests.
#[derive(Debug)]
pub struct Expect<E> {
    pub request: ExpectedRequest,
    pub response: Response<E>,
}

impl<E> Expect<E> {
    pub fn get(uri: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Get {
                id: uri.to_string().into(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }
    pub fn expand(uri: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Expand {
                id: uri.to_string().into(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }
    pub fn update(uri: impl Display, request: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Update {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }

    pub fn update_task(uri: impl Display, request: impl Display, task: AsyncTask) -> Self {
        Expect {
            request: ExpectedRequest::UpdateTask {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
                task,
            },
            response: Ok(JsonValue::Null),
        }
    }

    pub fn update_empty(uri: impl Display, request: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::UpdateEmpty {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
            },
            response: Ok(JsonValue::Null),
        }
    }

    pub fn create(uri: impl Display, request: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Create {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }

    pub fn create_task(uri: impl Display, request: impl Display, task: AsyncTask) -> Self {
        Expect {
            request: ExpectedRequest::CreateTask {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
                task,
            },
            response: Ok(JsonValue::Null),
        }
    }

    pub fn create_empty(uri: impl Display, request: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::CreateEmpty {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
            },
            response: Ok(JsonValue::Null),
        }
    }

    pub fn create_session(
        uri: impl Display,
        request: impl Display,
        response: impl Display,
        auth_token: impl Display,
        location: impl Display,
    ) -> Self {
        Expect {
            request: ExpectedRequest::CreateSession {
                id: uri.to_string().into(),
                request: from_str(&request.to_string()).expect("invalid json"),
                auth_token: auth_token.to_string(),
                location: location.to_string().into(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }
    pub fn action(uri: impl Display, request: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Action {
                target: ActionTarget::new(uri.to_string()),
                request: from_str(&request.to_string()).expect("invalid json"),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }

    pub fn multipart_update(
        uri: impl Display,
        request: impl Display,
        file_name: impl Display,
        response: impl Display,
    ) -> Self {
        Expect {
            request: ExpectedRequest::MultipartUpdate {
                uri: uri.to_string(),
                request: from_str(&request.to_string()).expect("invalid json"),
                file_name: file_name.to_string(),
                oem_parts: Vec::new(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }

    pub fn multipart_update_with_oem_parts<I, P>(
        uri: impl Display,
        request: impl Display,
        file_name: impl Display,
        oem_parts: I,
        response: impl Display,
    ) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Display,
    {
        Expect {
            request: ExpectedRequest::MultipartUpdate {
                uri: uri.to_string(),
                request: from_str(&request.to_string()).expect("invalid json"),
                file_name: file_name.to_string(),
                oem_parts: oem_parts.into_iter().map(|part| part.to_string()).collect(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }

    #[cfg(feature = "update-service-deprecated")]
    pub fn http_push_uri_update(uri: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::HttpPushUriUpdate {
                uri: uri.to_string(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }

    pub fn delete(uri: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Delete {
                id: uri.to_string().into(),
            },
            response: Ok(JsonValue::Null),
        }
    }

    pub fn delete_task(uri: impl Display, task: AsyncTask) -> Self {
        Expect {
            request: ExpectedRequest::DeleteTask {
                id: uri.to_string().into(),
                task,
            },
            response: Ok(JsonValue::Null),
        }
    }

    pub fn stream(uri: impl Display, response: impl Display) -> Self {
        Expect {
            request: ExpectedRequest::Stream {
                uri: uri.to_string(),
            },
            response: Ok(from_str(&response.to_string()).expect("invalid json")),
        }
    }
}
