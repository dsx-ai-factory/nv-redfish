// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Replies a platform gives to a write it accepted in a form the transport
//! reads as a failure: a `200` whose body is not a standard success.
//!
//! HPE iLO answers writes with an `error` coded `iLO.0.10.ExtendedInfo`,
//! "See @Message.ExtendedInfo for more information.", whose messages say
//! how it went. A success names a standard outcome, as `Base.1.4.Success`.
//! A change to a resource's properties that iLO accepted but has not yet
//! put in effect, or a value it already held, has messages from iLO's own
//! registry instead: a BIOS settings write or BIOS reset
//! `iLO.2.15.SystemResetRequired`, a `DateTime` write
//! `iLO.2.15.ResetRequired`. iLO gives failures such as
//! `InvalidOperationForSystemState` the same `Warning` or `OK` severity, so
//! of its own messages only those its registry documents as these outcomes
//! count.
//!
//! Lenovo XCC 3 answers a BIOS update it applied with `200` and a body that
//! names only `@odata.context` and `@odata.type`, which is neither the
//! resource nor a message.

use nv_redfish_core::BmcError;
use nv_redfish_core::BmcErrorClass;
use nv_redfish_core::ModificationResponse;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value;

use crate::BmcQuirks;

/// Reads a reply body as one platform's success, returning it as a document.
type ReplyReader = fn(&str) -> Option<Value>;

/// The success replies a platform's writes can come back as.
pub struct SuccessReplies {
    /// The readers of the platform's success replies.
    readers: Vec<ReplyReader>,
}

impl SuccessReplies {
    /// The success replies of the classified platform `quirks`.
    pub fn new(quirks: &BmcQuirks) -> Self {
        let mut readers: Vec<ReplyReader> = Vec::new();
        if quirks.bug_success_as_extended_info_error() {
            readers.push(ilo_success);
        }
        Self { readers }
    }

    /// The success reply a write's `error` is: a `200` whose body one of the
    /// platform's readers reads as its success.
    pub fn read(&self, error: &impl BmcError) -> Option<Value> {
        if error.error_class() != (BmcErrorClass::HttpResponse { status: 200 }) {
            return None;
        }
        let body = error.response_body()?;
        self.readers.iter().find_map(|read| read(body))
    }
}

/// The keys of the iLO messages that report a property change iLO accepted
/// or a value it already held; the same in the iLO registries of iLO 5 to
/// iLO 7.
const ILO_SUCCESS_MESSAGES: [&str; 5] = [
    "SystemResetRequired",
    "ResetRequired",
    "DeviceResetRequired",
    "Accepted",
    "PropertyValueAlreadySet",
];

/// The keys of the messages that name a standard outcome, in any registry.
const OUTCOMES: [&str; 3] = ["Success", "Created", "NoOperation"];

/// The registry and key of a message ID or status `code`: its first and
/// last segments, as `iLO` and `ExtendedInfo` in `iLO.0.10.ExtendedInfo`.
fn registry_and_key(id: &str) -> (&str, &str) {
    let registry = id.split('.').next().unwrap_or(id);
    let key = id.rsplit_once('.').map_or(id, |(_, key)| key);
    (registry, key)
}

/// `body` as a document when it is iLO's reply that a write succeeded: an
/// `error` coded `ExtendedInfo` whose `@Message.ExtendedInfo` has no
/// `Critical` message and either names one of the [`OUTCOMES`], beside any
/// other messages, or lists only [`ILO_SUCCESS_MESSAGES`] from the registry
/// of that code.
fn ilo_success(body: &str) -> Option<Value> {
    let reply: Value = serde_json::from_str(body).ok()?;
    let error = reply.get("error")?;
    let (registry, key) = registry_and_key(error.get("code")?.as_str()?);
    if key != "ExtendedInfo" {
        return None;
    }
    let ids = error
        .get("@Message.ExtendedInfo")?
        .as_array()?
        .iter()
        .map(|message| {
            let critical = ["MessageSeverity", "Severity"]
                .iter()
                .any(|property| message.get(property).and_then(Value::as_str) == Some("Critical"));
            if critical {
                return None;
            }
            message.get("MessageId")?.as_str().map(registry_and_key)
        })
        .collect::<Option<Vec<_>>>()?;
    let names_outcome = ids.iter().any(|(_, key)| OUTCOMES.contains(key));
    let only_ilo_successes = !ids.is_empty()
        && ids.iter().all(|(message_registry, key)| {
            *message_registry == registry && ILO_SUCCESS_MESSAGES.contains(key)
        });
    (names_outcome || only_ilo_successes).then_some(reply)
}

/// The keys of the body Lenovo XCC 3 answers an applied BIOS update with.
const ANNOTATION_ONLY_REPLY_KEYS: [&str; 2] = ["@odata.context", "@odata.type"];

/// An update's reply, which Lenovo XCC 3 can send as OData annotations alone.
pub enum UpdateReply<R> {
    /// The reply the caller asked for.
    Resource(R),
    /// A body naming only `@odata.context` and `@odata.type`.
    AnnotationsOnly,
}

impl<R> UpdateReply<R> {
    /// The response to the update, with an annotation-only reply read as an
    /// empty one.
    pub fn into_response(response: ModificationResponse<Self>) -> ModificationResponse<R> {
        match response {
            ModificationResponse::Entity(Self::Resource(resource)) => {
                ModificationResponse::Entity(resource)
            }
            ModificationResponse::Entity(Self::AnnotationsOnly) | ModificationResponse::Empty => {
                ModificationResponse::Empty
            }
            ModificationResponse::Task(task) => ModificationResponse::Task(task),
            ModificationResponse::Message(message) => ModificationResponse::Message(message),
        }
    }
}

impl<'de, R: for<'a> Deserialize<'a>> Deserialize<'de> for UpdateReply<R> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let reply = Value::deserialize(deserializer)?;
        let annotations_only = reply.as_object().is_some_and(|reply| {
            !reply.is_empty()
                && reply
                    .keys()
                    .all(|key| ANNOTATION_ONLY_REPLY_KEYS.contains(&key.as_str()))
        });
        if annotations_only {
            return Ok(Self::AnnotationsOnly);
        }
        // Keep the field path a direct decode of `R` would report.
        serde_path_to_error::deserialize(&reply)
            .map(Self::Resource)
            .map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn reply(messages: &[&str]) -> Value {
        json!({"error": {
            "code": "iLO.0.10.ExtendedInfo",
            "message": "See @Message.ExtendedInfo for more information.",
            "@Message.ExtendedInfo": messages
                .iter()
                .map(|id| json!({"MessageId": id, "MessageSeverity": "Warning"}))
                .collect::<Vec<_>>(),
        }})
    }

    #[test]
    fn only_standard_outcomes_and_ilos_documented_successes_are_successes() {
        for (scenario, body, success) in [
            ("a standard outcome", reply(&["Base.1.4.Success"]), true),
            (
                "a standard outcome beside another message",
                reply(&["Base.1.4.Success", "Base.1.4.PropertyValueModified"]),
                true,
            ),
            (
                "a standard outcome beside a critical message",
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "@Message.ExtendedInfo": [
                        {"MessageId": "Base.1.4.Success"},
                        {"MessageId": "Base.1.4.InternalError", "MessageSeverity": "Critical"}
                    ]
                }}),
                false,
            ),
            (
                "a message without an ID",
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "@Message.ExtendedInfo": [
                        {"MessageId": "Base.1.4.Success"},
                        {"Message": "Done."}
                    ]
                }}),
                false,
            ),
            (
                "reset required, as iLO 5 answers a BIOS reset",
                reply(&["iLO.2.15.SystemResetRequired"]),
                true,
            ),
            (
                "a value already held beside a reset required",
                reply(&["iLO.2.44.PropertyValueAlreadySet", "iLO.2.44.ResetRequired"]),
                true,
            ),
            (
                "a failure iLO gives as mild a severity",
                reply(&["iLO.2.15.InvalidOperationForSystemState"]),
                false,
            ),
            (
                "a success beside a message of another registry",
                reply(&[
                    "iLO.2.15.SystemResetRequired",
                    "Base.1.4.ActionParameterNotSupported",
                ]),
                false,
            ),
            ("no messages", reply(&[]), false),
            (
                "another code",
                json!({"error": {
                    "code": "Base.1.8.GeneralError",
                    "@Message.ExtendedInfo": [{"MessageId": "iLO.2.15.SystemResetRequired"}]
                }}),
                false,
            ),
        ] {
            let text = body.to_string();
            assert_eq!(ilo_success(&text), success.then_some(body), "{scenario}");
        }
        assert_eq!(ilo_success("not json"), None);
    }

    /// Only the body XCC 3 answered a BIOS update with names no resource; a
    /// resource, or an empty object, is decoded as the caller asked.
    #[test]
    fn only_an_annotation_only_body_names_no_resource() {
        for (scenario, body, names_resource) in [
            (
                "XCC 3 BIOS update reply",
                json!({
                    "@odata.context": "/redfish/v1/$metadata#Bios.Bios",
                    "@odata.type": "#Bios.v1_2_3.Bios"
                }),
                false,
            ),
            (
                "a resource",
                json!({
                    "@odata.id": "/redfish/v1/Systems/1/Bios",
                    "@odata.type": "#Bios.v1_2_3.Bios"
                }),
                true,
            ),
            ("an empty object", json!({}), true),
        ] {
            let reply: UpdateReply<Value> =
                serde_json::from_value(body).expect("every body decodes as a Value");
            assert_eq!(
                matches!(reply, UpdateReply::Resource(_)),
                names_resource,
                "{scenario}"
            );
        }
    }

    /// A reply that is neither annotations alone nor the requested type
    /// reports where it differs, as a direct decode would.
    #[test]
    fn a_mismatched_reply_keeps_its_field_path() {
        #[derive(Debug, Deserialize)]
        struct Requested {
            #[allow(dead_code)]
            status: Status,
        }
        #[derive(Debug, Deserialize)]
        struct Status {
            #[allow(dead_code)]
            health: u8,
        }

        let error = serde_json::from_value::<UpdateReply<Requested>>(json!({
            "status": { "health": "OK" }
        }))
        .err()
        .expect("a string health is not the requested type");
        assert!(error.to_string().contains("status.health"), "{}", error);
    }
}
