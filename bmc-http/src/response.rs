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

//! Decoding of the responses to requests that modify a service.
//!
//! These functions take the parts of an HTTP response, so any
//! [`HttpClient`](crate::HttpClient) decodes with the same rules, vendor
//! fixes included. The reqwest client uses them for its own responses.

use std::error::Error as StdError;
use std::fmt;
use std::time::Duration;

use http::header;
use http::HeaderMap;
use http::StatusCode;
use nv_redfish_core::odata::ODataType;
use nv_redfish_core::AsyncTask;
use nv_redfish_core::BmcError;
use nv_redfish_core::BmcErrorClass;
use nv_redfish_core::DecodeError;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use serde::de::DeserializeOwned;
use serde_json::Value;
use url::Url;

/// A response that does not decode as the outcome of a request.
#[derive(Debug)]
#[non_exhaustive]
pub enum ModificationDecodeError {
    /// The status, a header or the body is not one the request allows.
    InvalidResponse {
        /// URL of the response.
        url: Url,
        /// Status of the response.
        status: StatusCode,
        /// What is wrong, or the body of an unsuccessful response.
        text: String,
    },
    /// The body is not JSON.
    Json(serde_json::Error),
    /// The body deserialized neither into the requested type nor as a
    /// message envelope.
    Decode(DecodeError),
}

impl fmt::Display for ModificationDecodeError {
    /// Formats the decode error with its HTTP context or JSON path.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidResponse { url, status, text } => write!(
                f,
                "Invalid HTTP response - url: {url} status: {status} text: {text}"
            ),
            Self::Json(e) => write!(f, "JSON Decode error: {e}"),
            Self::Decode(e) => write!(
                f,
                "JSON deserialization error at line {} column {} path {}: {e}",
                e.inner().line(),
                e.inner().column(),
                e.path(),
            ),
        }
    }
}

impl StdError for ModificationDecodeError {
    /// Returns the underlying JSON or deserialization error, if any.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::InvalidResponse { .. } => None,
            Self::Json(e) => Some(e),
            Self::Decode(e) => Some(e.inner()),
        }
    }
}

impl BmcError for ModificationDecodeError {
    /// Classifies the error as an HTTP response error or a response parse error.
    fn error_class(&self) -> BmcErrorClass {
        match self {
            Self::InvalidResponse { status, .. } => BmcErrorClass::HttpResponse {
                status: status.as_u16(),
            },
            Self::Json(_) | Self::Decode(_) => BmcErrorClass::ResponseParse,
        }
    }

    /// Extracts the inner [`DecodeError`] when the failure is a schema mismatch.
    fn into_decode_error(self) -> Result<DecodeError, Self> {
        match self {
            Self::Decode(error) => Ok(error),
            other => Err(other),
        }
    }
}

/// The outcome that a Redfish success message reports, by the key of its
/// message in the Base message registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SuccessCode {
    /// `Success`: the request completed.
    Success,
    /// `Created`: the request created a resource.
    Created,
    /// `NoOperation`: the request changed nothing, because nothing needed
    /// to change.
    NoOperation,
}

impl SuccessCode {
    /// The code a message ID or an error `code` names by its key, as in
    /// `Base.1.8.Success`, whatever its registry and version.
    fn of(message_id: &str) -> Option<Self> {
        match key(message_id) {
            "Success" => Some(Self::Success),
            "Created" => Some(Self::Created),
            "NoOperation" => Some(Self::NoOperation),
            _ => None,
        }
    }

    /// Which code a body reports when its messages name several: `Created`,
    /// then `Success`, then `NoOperation`.
    const fn precedence(self) -> u8 {
        match self {
            Self::NoOperation => 0,
            Self::Success => 1,
            Self::Created => 2,
        }
    }

    /// Whether a message naming this code may have `severity`: the one the
    /// Base registry gives it or a lesser one. `Success` and `Created` are
    /// `OK` there, and `NoOperation` is a `Warning`.
    fn allows(self, severity: &str) -> bool {
        match severity {
            "OK" => true,
            "Warning" => self == Self::NoOperation,
            _ => false,
        }
    }
}

/// The key of a message ID or a status `code`: its last segment, as
/// `Success` in `Base.1.8.Success`.
fn key(id: &str) -> &str {
    id.rsplit_once('.').map_or(id, |(_, key)| key)
}

/// The success that a response body reports when it is a success message
/// instead of a resource or an action's result.
///
/// DSP0266 lets an action without a result answer with an error-shaped
/// body whose message indicates success, and services answer other requests
/// with messages alone too. Only the `code` of a status object and the
/// `MessageId`, `MessageSeverity` and `Severity` of a message are read, so a
/// property that does not fit the schema, such as a numeric `MessageArgs`,
/// changes nothing.
///
/// - A body with `error`, or with `Success` as Supermicro BMCs use, reports
///   the `code` of that object, with or without its `message`, as in
///   `{"error": {"code": "Base.1.8.Success", "message": "..."}}`. A code
///   whose key is `ExtendedInfo`, as HPE iLO's `iLO.0.10.ExtendedInfo`,
///   reports what the object's own `@Message.ExtendedInfo` names. Any other
///   code, or an object without one, is not a success, whatever messages
///   are beside it.
/// - Otherwise, a body with `@Message.ExtendedInfo` reports what its
///   messages name: [`SuccessCode::Created`] when one is `Created`, else
///   [`SuccessCode::Success`] when one is `Success`, else
///   [`SuccessCode::NoOperation`] when one is `NoOperation`. When none
///   names an outcome, as in an empty array or a message from another
///   registry, the body is [`SuccessCode::Success`], which its HTTP status
///   reports.
///
/// A message with a `Critical` `MessageSeverity` or `Severity`, or without a
/// `MessageId`, makes the body not a success, as does
/// `@Message.ExtendedInfo` that is not an array.
///
/// A success can come with warnings and other messages. A caller that
/// must not count those, such as for a host reset, inspects the
/// [`ModificationResponse::Message`] body (or [`ModificationResponse::Entity`]
/// when decoding into [`Value`]) with [`strict_success_code`].
#[must_use]
pub fn success_code(body: &Value) -> Option<SuccessCode> {
    report(body).map(|report| report.code)
}

/// The success that a response body reports when it is a success message
/// that reports nothing else, as [`success_code`] reads it.
///
/// A status object's `code` or a message's `MessageId` must name the
/// outcome. Every message must name `Success`, `Created` or `NoOperation`
/// with no severity worse than the Base registry gives it: `OK`, or
/// `Warning` for `NoOperation`. `NoOperation` must not be named beside
/// `Success` or `Created`, as when part of a request did nothing.
///
/// So a warning, a message from another registry such as Dell's
/// `IDRAC.2.9.SYS413`, or an empty `@Message.ExtendedInfo` makes this
/// `None` although [`success_code`] reads a success. A caller that must
/// know that an action did what it asked, such as a host reset, inspects
/// the [`ModificationResponse::Message`] body (or
/// [`ModificationResponse::Entity`] when decoding into [`Value`]) and accepts
/// only `Some(SuccessCode::Success)`.
#[must_use]
pub fn strict_success_code(body: &Value) -> Option<SuccessCode> {
    report(body)
        .filter(|report| !report.qualified)
        .map(|report| report.code)
}

/// What a success message reports.
struct Report {
    /// The outcome.
    code: SuccessCode,
    /// Whether it reports anything besides: it does not name its outcome, or
    /// it has a message [`strict_success_code`] does not allow.
    qualified: bool,
}

/// What the status codes and messages of a body name.
#[derive(Clone, Copy, Default)]
struct Names {
    /// It has `@Message.ExtendedInfo`.
    listed: bool,
    /// The code named, by [`SuccessCode::precedence`].
    code: Option<SuccessCode>,
    /// `NoOperation` is named.
    no_operation: bool,
    /// A message names no outcome or has a severity that its outcome does
    /// not allow.
    other: bool,
}

impl Names {
    /// Records that `code` is named.
    fn name(&mut self, code: SuccessCode) {
        self.no_operation |= code == SuccessCode::NoOperation;
        if self
            .code
            .is_none_or(|named| named.precedence() < code.precedence())
        {
            self.code = Some(code);
        }
    }

    /// Adds what `other` names.
    fn add(&mut self, other: Self) {
        if let Some(code) = other.code {
            self.name(code);
        }
        self.no_operation |= other.no_operation;
        self.other |= other.other;
    }
}

/// What the `@Message.ExtendedInfo` of `value` names, or `None` when its
/// messages make the body not a success.
fn messages(value: &Value) -> Option<Names> {
    let mut names = Names::default();
    let Some(messages) = value
        .get("@Message.ExtendedInfo")
        .filter(|messages| !messages.is_null())
    else {
        return Some(names);
    };
    names.listed = true;
    for message in messages.as_array()? {
        let severities = ["MessageSeverity", "Severity"]
            .map(|property| message.get(property).and_then(Value::as_str));
        if severities.contains(&Some("Critical")) {
            return None;
        }
        match SuccessCode::of(message.get("MessageId")?.as_str()?) {
            Some(code) => {
                names.name(code);
                names.other |= severities
                    .iter()
                    .flatten()
                    .any(|severity| !code.allows(severity));
            }
            None => names.other = true,
        }
    }
    Some(names)
}

/// Reads `body` as a success message; see [`success_code`].
fn report(body: &Value) -> Option<Report> {
    let body_messages = messages(body)?;
    let mut named = body_messages;
    let mut status_codes = Names::default();
    for status in ["error", "Success"]
        .iter()
        .filter_map(|property| body.get(property))
        .filter(|status| !status.is_null())
    {
        let status_messages = messages(status)?;
        let code = status.get("code")?.as_str()?;
        let code = match SuccessCode::of(code) {
            Some(code) => code,
            // HPE iLO answers with `iLO.0.10.ExtendedInfo`, "See
            // @Message.ExtendedInfo for more information."
            None if key(code) == "ExtendedInfo" => status_messages.code?,
            None => return None,
        };
        status_codes.name(code);
        named.add(status_messages);
    }
    named.add(status_codes);

    let code = match status_codes.code {
        Some(code) => Some(code),
        None if body_messages.listed => body_messages.code,
        None => return None,
    };
    let mixed = if code == Some(SuccessCode::NoOperation) {
        named.code != Some(SuccessCode::NoOperation)
    } else {
        named.no_operation
    };
    Some(Report {
        code: code.unwrap_or(SuccessCode::Success),
        qualified: code.is_none() || named.other || mixed,
    })
}

/// Whether `body` is a Redfish status or message envelope: it has `error`,
/// `Success`, or `@Message.ExtendedInfo`.
fn is_message_envelope(body: &Value) -> bool {
    ["error", "Success", "@Message.ExtendedInfo"]
        .iter()
        .any(|property| body.get(property).is_some_and(|value| !value.is_null()))
}

/// Decodes the response to a request that modifies a service: an action, a
/// create, an update, a delete or an upload.
///
/// `url` is the URL of the response, after any redirect, which a relative
/// `Location` resolves against. `body` is the whole response body.
///
/// - `204 No Content` is [`ModificationResponse::Empty`].
/// - `202 Accepted` is a [`ModificationResponse::Task`] at its `Location`,
///   to poll after its `Retry-After` seconds, with the same-origin `Task`
///   resource its body represents, if any. Without a `Location`, it is at
///   that `Task`, which a poll reads instead of a task monitor.
/// - `200 OK` or `201 Created` is the body as `T`, with the `ETag` header as
///   the `@odata.etag` of a body that names its `@odata.id`. A body that is
///   not `T` but a success message (see [`success_code`]) is
///   [`ModificationResponse::Message`], or the resource its `Location` names
///   when that is a `T`. An empty body is that resource, or
///   [`ModificationResponse::Empty`] without a `Location`.
///
/// A success message that does not deserialize as `T` is
/// [`ModificationResponse::Message`] whatever it reports, `NoOperation` and
/// warnings included. Because deserializing `T` takes precedence, a caller
/// that decodes into [`Value`] (or another type that accepts message objects)
/// receives [`ModificationResponse::Entity`] instead. A caller that must tell
/// outcomes apart inspects the [`ModificationResponse::Message`] body—or
/// [`ModificationResponse::Entity`] when `T` is [`Value`]—with
/// [`success_code`] or [`strict_success_code`].
///
/// # Errors
///
/// - [`ModificationDecodeError::InvalidResponse`] for an unsuccessful status
///   or a `200` or `201` message envelope that is not a success, with the
///   body as its text; another successful status; a `202` with neither a
///   `Location` nor a same-origin `Task` body; or a `Location` that does not
///   name a resource of the same origin.
/// - [`ModificationDecodeError::Json`] when a `200` or `201` body is not JSON.
/// - [`ModificationDecodeError::Decode`] when a `200` or `201` body is neither
///   `T` nor a message envelope, or an empty one names a `Location` that is
///   not a `T`.
pub fn decode_modification<T>(
    url: &Url,
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<ModificationResponse<T>, ModificationDecodeError>
where
    T: DeserializeOwned,
{
    if !status.is_success() {
        return Err(ModificationDecodeError::InvalidResponse {
            url: url.clone(),
            status,
            text: String::from_utf8_lossy(body).into_owned(),
        });
    }

    // Resolve the header once, but defer propagating its error until a
    // status branch actually uses Location. A malformed, irrelevant
    // Location must not turn a valid 204 or entity-bearing 200/201 into an
    // error.
    let location = location_from_headers(headers, url, status);

    match status {
        StatusCode::NO_CONTENT => Ok(ModificationResponse::Empty),
        StatusCode::ACCEPTED => {
            let task = task_from_body(body, url);
            // DSP0266 names the task monitor in Location. A service that
            // names none can still return the Task, which can be polled.
            let Some(task_location) = location?.or_else(|| task.clone()) else {
                return Err(ModificationDecodeError::InvalidResponse {
                    url: url.clone(),
                    status,
                    text: String::from("202 Accepted without Location header"),
                });
            };

            Ok(ModificationResponse::Task(AsyncTask {
                location: task_location.into(),
                retry_after: retry_after_from_headers(headers),
                task,
            }))
        }
        StatusCode::OK | StatusCode::CREATED => {
            if !body.is_empty() {
                let mut value: Value =
                    serde_json::from_slice(body).map_err(ModificationDecodeError::Json)?;

                if value.get("@odata.id").is_some() {
                    if let Some(etag) = etag_from_headers(headers) {
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
                        if success_code(&value).is_some() {
                            // A success message is not the requested
                            // representation. Preserve an accompanying
                            // resource Location in the same way as an
                            // empty 200/201 response, or return the
                            // message body so callers can inspect it.
                            if let Some(location) = location? {
                                if let Ok(entity) = entity_reference(&location) {
                                    return Ok(ModificationResponse::Entity(entity));
                                }
                            }
                            Ok(ModificationResponse::Message(value))
                        } else if is_message_envelope(&value) {
                            // A message envelope that is not a success, such
                            // as a 200 with a Critical message or an error
                            // code, carries the BMC's error in its body.
                            Err(ModificationDecodeError::InvalidResponse {
                                url: url.clone(),
                                status,
                                text: String::from_utf8_lossy(body).into_owned(),
                            })
                        } else {
                            // The response was successful JSON, but it was
                            // neither the caller-selected type nor a message
                            // envelope.
                            Err(ModificationDecodeError::Decode(err))
                        }
                    }
                };
            }

            if let Some(location) = location? {
                return entity_reference(&location)
                    .map(ModificationResponse::Entity)
                    .map_err(ModificationDecodeError::Decode);
            }

            Ok(ModificationResponse::Empty)
        }
        _ => Err(ModificationDecodeError::InvalidResponse {
            url: url.clone(),
            status,
            text: format!("Unexpected successful status code: {status}"),
        }),
    }
}

/// Decodes the response to polling the URI of an asynchronous operation.
///
/// A `202 Accepted` is still running: a [`ModificationResponse::Task`] at its
/// `Location`, or at the polled URI when it names none, as pending responses
/// often omit it. A `204 No Content` naming a `Location` is that resource, as
/// an empty `200` with one is. Any other response decodes as
/// [`decode_modification`] does.
///
/// # Errors
///
/// As [`decode_modification`], and
/// [`ModificationDecodeError::InvalidResponse`] when the `Location` of a
/// `202` does not name a resource of the same origin.
pub fn decode_poll<T>(
    url: &Url,
    status: StatusCode,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<ModificationResponse<T>, ModificationDecodeError>
where
    T: DeserializeOwned,
{
    match status {
        StatusCode::ACCEPTED => {
            let location = location_from_headers(headers, url, status)?
                .unwrap_or_else(|| odata_path_from_url(url).into());
            Ok(ModificationResponse::Task(AsyncTask {
                location: location.into(),
                retry_after: retry_after_from_headers(headers),
                task: task_from_body(body, url),
            }))
        }
        StatusCode::NO_CONTENT => location_from_headers(headers, url, status)?.map_or(
            Ok(ModificationResponse::Empty),
            |location| {
                entity_reference(&location)
                    .map(ModificationResponse::Entity)
                    .map_err(ModificationDecodeError::Decode)
            },
        ),
        _ => decode_modification(url, status, headers, body),
    }
}

/// A `T` that is only a reference to the resource at `location`.
fn entity_reference<T: DeserializeOwned>(location: &ODataId) -> Result<T, DecodeError> {
    serde_path_to_error::deserialize(serde_json::json!({ "@odata.id": location }))
}

/// Resolve a Redfish `Location` header into a same-origin path and query.
///
/// HTTP defines `Location` as a URI reference, so values may be absolute,
/// root-relative, path-relative, or query-only. Resolution must use the final
/// response URL; treating a relative value as a path rooted at the configured
/// BMC endpoint can target a different resource. The returned `ODataId` keeps
/// only path and query because fragments are not sent in subsequent HTTP
/// requests and transport always uses the configured BMC origin.
pub fn location_from_headers(
    headers: &HeaderMap,
    response_url: &Url,
    status: StatusCode,
) -> Result<Option<ODataId>, ModificationDecodeError> {
    let Some(value) = headers.get(header::LOCATION) else {
        return Ok(None);
    };

    let raw = value
        .to_str()
        .map_err(|_| ModificationDecodeError::InvalidResponse {
            url: response_url.clone(),
            status,
            text: String::from("Location header is not valid text"),
        })?;

    same_origin_reference(raw, response_url, status, "Location header").map(Some)
}

/// Resolves the URI reference `raw`, which `what` names in errors, against
/// `response_url` into a same-origin path and query.
fn same_origin_reference(
    raw: &str,
    response_url: &Url,
    status: StatusCode,
    what: &str,
) -> Result<ODataId, ModificationDecodeError> {
    let invalid_response = |problem: &str| ModificationDecodeError::InvalidResponse {
        url: response_url.clone(),
        status,
        text: format!("{what} {problem}"),
    };

    let raw = raw.trim();

    // Joining either value would resolve back to the response resource, which
    // cannot identify a newly created session or asynchronous task monitor.
    if raw.is_empty() || raw.starts_with('#') {
        return Err(invalid_response("does not identify a resource"));
    }

    let resolved = response_url
        .join(raw)
        .map_err(|_| invalid_response("is not a valid URI reference"))?;

    // Redfish follow-up requests carry BMC credentials. Reject another origin
    // before reducing the URL to an OData path and losing that distinction.
    if resolved.origin() != response_url.origin() {
        return Err(invalid_response("resolves to a different origin"));
    }

    Ok(odata_path_from_url(&resolved).into())
}

/// Extracts the path and optional query string of `url` as an OData path.
fn odata_path_from_url(url: &Url) -> String {
    let mut path_and_query = url.path().to_string();

    // Preserve the query separately from the path so later polling or deletion
    // sends it as a query instead of percent-encoded path text.
    if let Some(query) = url.query() {
        path_and_query.push('?');
        path_and_query.push_str(query);
    }

    path_and_query
}

/// Extracts the `ETag` header from `headers` as an [`ODataETag`].
pub fn etag_from_headers(headers: &HeaderMap) -> Option<ODataETag> {
    headers
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .map(|v| v.to_string().into())
}

/// The `Task` resource a `202 Accepted` body represents, resolved to a
/// same-origin path and query against `response_url`, if the body is one.
fn task_from_body(body: &[u8], response_url: &Url) -> Option<ODataId> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let is_task = ODataType::parse_from(&value).is_some_and(|odata_type| {
        odata_type.namespace.first() == Some(&"Task") && odata_type.type_name == "Task"
    });
    if !is_task {
        return None;
    }
    let id = value.get("@odata.id")?.as_str()?;
    same_origin_reference(id, response_url, StatusCode::ACCEPTED, "Task @odata.id").ok()
}

/// Parses a numeric `Retry-After` header from `headers` as a delay in seconds.
fn retry_after_from_headers(headers: &HeaderMap) -> Option<Duration> {
    headers
        .get(header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok())
        // RFC 9110 defines the numeric Retry-After form as delay-seconds.
        // This helper handles that form and leaves HTTP-date support out of scope.
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Sets `@odata.etag` on a JSON object `body` from an HTTP `ETag` header.
pub fn inject_etag(etag: &ODataETag, body: &mut Value) {
    if let Some(obj) = body.as_object_mut() {
        let etag_value = Value::String(etag.to_string());

        // Handles both absent and null values
        obj.entry("@odata.etag")
            .and_modify(|v| *v = etag_value.clone())
            .or_insert(etag_value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::error::Error as StdError;

    use http::HeaderValue;
    use serde_json::json;

    /// A resource reference, as a create that names its `Location` returns.
    #[derive(Debug, serde::Deserialize)]
    struct Reference {
        #[serde(rename = "@odata.id")]
        odata_id: String,
    }

    /// A resource with an ETag.
    #[derive(Debug, serde::Deserialize)]
    struct Resource {
        #[serde(rename = "@odata.id")]
        odata_id: String,
        #[serde(rename = "@odata.etag")]
        odata_etag: Option<String>,
    }

    /// Returns the default action URL used by modification decode tests.
    fn url() -> Url {
        Url::parse("https://bmc.example/redfish/v1/Systems/1/Actions/ComputerSystem.Reset")
            .expect("valid URL")
    }

    /// Builds a [`HeaderMap`] from static header name/value pairs.
    fn headers(pairs: &[(header::HeaderName, &'static str)]) -> HeaderMap {
        pairs
            .iter()
            .map(|(name, value)| (name.clone(), HeaderValue::from_static(value)))
            .collect()
    }

    /// Decodes a JSON `body` with `status` and `headers` against the default test URL.
    fn decode<T: DeserializeOwned>(
        status: StatusCode,
        headers: &HeaderMap,
        body: &Value,
    ) -> Result<ModificationResponse<T>, ModificationDecodeError> {
        decode_modification(&url(), status, headers, body.to_string().as_bytes())
    }

    /// Verifies that [`success_code`] reads a status object, standard or
    /// vendor, by its code.
    #[test]
    fn status_objects_report_their_code() {
        for (body, code) in [
            // DSP0266: an error response whose message indicates success.
            (
                json!({"error": {"code": "Base.1.8.Success", "message": "Done."}}),
                Some(SuccessCode::Success),
            ),
            // Supermicro: the status under `Success`.
            (
                json!({"Success": {"code": "Base.1.10.Success", "message": "Done."}}),
                Some(SuccessCode::Success),
            ),
            (
                json!({"error": {"code": "Base.1.8.Created", "message": "Created."}}),
                Some(SuccessCode::Created),
            ),
            (
                json!({"error": {"code": "Base.1.8.NoOperation", "message": "Nothing."}}),
                Some(SuccessCode::NoOperation),
            ),
            (
                json!({"error": {"code": "Base.1.8.GeneralError", "message": "Failed."}}),
                None,
            ),
            // The code decides, with or without the message.
            (
                json!({"Success": {"code": "Base.1.10.Success"}}),
                Some(SuccessCode::Success),
            ),
            (
                json!({"error": {"code": "Base.1.8.Success"}}),
                Some(SuccessCode::Success),
            ),
            (
                json!({"Success": {"code": "Base.1.10.Success", "Message": "Done."}}),
                Some(SuccessCode::Success),
            ),
            (
                json!({
                    "Success": {"code": "Base.1.10.Success"},
                    "@Message.ExtendedInfo": [{"MessageId": "Base.1.10.Success"}]
                }),
                Some(SuccessCode::Success),
            ),
            (
                json!({
                    "Success": {
                        "code": "Base.1.10.Success",
                        "@Message.ExtendedInfo": [{"MessageId": "Base.1.10.Success"}]
                    }
                }),
                Some(SuccessCode::Success),
            ),
            (
                json!({
                    "error": {"code": "Base.1.8.Success", "message": "Done."},
                    "@Message.ExtendedInfo": null
                }),
                Some(SuccessCode::Success),
            ),
            // HPE iLO: a code that refers to the object's own messages.
            (
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "message": "See @Message.ExtendedInfo for more information.",
                    "@Message.ExtendedInfo": [{"MessageId": "Base.1.4.Success"}]
                }}),
                Some(SuccessCode::Success),
            ),
            (
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "@Message.ExtendedInfo": [{"MessageId": "Base.1.4.Created"}]
                }}),
                Some(SuccessCode::Created),
            ),
            (
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "@Message.ExtendedInfo": [{
                        "MessageId": "Base.1.4.NoOperation",
                        "MessageSeverity": "Warning"
                    }]
                }}),
                Some(SuccessCode::NoOperation),
            ),
        ] {
            assert_eq!(success_code(&body), code, "{body}");
        }
    }

    /// Verifies that [`success_code`] reads `@Message.ExtendedInfo` alone by
    /// the outcomes its messages name.
    #[test]
    fn extended_information_reports_what_it_names() {
        for (body, code) in [
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.13.0.Success",
                    "MessageSeverity": "OK"
                }]}),
                Some(SuccessCode::Success),
            ),
            (
                json!({"@Message.ExtendedInfo": [{"MessageId": "IDRAC.2.9.SYS413"}]}),
                Some(SuccessCode::Success),
            ),
            (
                json!({"@Message.ExtendedInfo": [
                    {"MessageId": "Base.1.8.Success"},
                    {"MessageId": "Base.1.8.Created"}
                ]}),
                Some(SuccessCode::Created),
            ),
            (
                json!({"@Message.ExtendedInfo": [{"MessageId": "Base.1.8.NoOperation"}]}),
                Some(SuccessCode::NoOperation),
            ),
            (
                json!({"@Message.ExtendedInfo": [
                    {"MessageId": "Base.1.8.NoOperation"},
                    {"MessageId": "Base.1.8.Success"}
                ]}),
                Some(SuccessCode::Success),
            ),
            // A message from another registry does not hide NoOperation.
            (
                json!({"@Message.ExtendedInfo": [
                    {"MessageId": "Base.1.8.NoOperation"},
                    {"MessageId": "IDRAC.2.9.SYS413"}
                ]}),
                Some(SuccessCode::NoOperation),
            ),
            (
                json!({"@Message.ExtendedInfo": []}),
                Some(SuccessCode::Success),
            ),
            // Only MessageId and the severities are read.
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.Success",
                    "MessageArgs": [1],
                    "MessageSeverity": "Informational",
                    "Oem": "unexpected"
                }]}),
                Some(SuccessCode::Success),
            ),
            (json!({"Result": "Accepted"}), None),
            (json!({}), None),
            (json!(null), None),
        ] {
            assert_eq!(success_code(&body), code, "{body}");
        }
    }

    /// Verifies that a critical message, with or without a `MessageId`, is
    /// not a success wherever it is.
    #[test]
    fn a_critical_message_is_not_a_success() {
        for body in [
            json!({"@Message.ExtendedInfo": [{
                "MessageId": "Base.1.8.InternalError",
                "MessageSeverity": "Critical"
            }]}),
            json!({"@Message.ExtendedInfo": [
                {"MessageId": "Base.1.8.Success", "Severity": "OK"},
                {"MessageId": "Base.1.8.InternalError", "Severity": "Critical"}
            ]}),
            json!({"error": {
                "code": "Base.1.8.Success",
                "message": "Done.",
                "@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.InternalError",
                    "MessageSeverity": "Critical"
                }]
            }}),
            json!({
                "error": {"code": "Base.1.8.Success", "message": "Done."},
                "@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.InternalError",
                    "Severity": "Critical"
                }]
            }),
            json!({
                "Success": {"code": "Base.1.10.Success"},
                "@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.10.InternalError",
                    "MessageSeverity": "Critical"
                }]
            }),
            json!({
                "Success": {
                    "code": "Base.1.10.Success",
                    "@Message.ExtendedInfo": [{
                        "MessageId": "Base.1.10.InternalError",
                        "Severity": "Critical"
                    }]
                }
            }),
            // A critical message without a MessageId is still critical.
            json!({"error": {
                "code": "Base.1.8.Success",
                "message": "Done.",
                "@Message.ExtendedInfo": [{"Severity": "Critical", "Message": "Failed."}]
            }}),
            json!({
                "Success": {"code": "Base.1.10.Success", "message": "Done."},
                "@Message.ExtendedInfo": [{"MessageSeverity": "Critical"}]
            }),
        ] {
            assert_eq!(success_code(&body), None, "{body}");
            assert_eq!(strict_success_code(&body), None, "{body}");
        }
    }

    /// Verifies that an error code, a status object without a code, and
    /// messages that cannot be read are not a success.
    #[test]
    fn an_error_code_or_an_unreadable_body_is_not_a_success() {
        for body in [
            // The code of an error decides, whatever the messages beside it or
            // in it, and with or without its message.
            json!({
                "error": {"code": "Base.1.8.GeneralError", "message": "Failed."},
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]
            }),
            json!({
                "error": {"code": "Base.1.8.GeneralError"},
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]
            }),
            json!({"error": {"code": "Base.1.8.GeneralError"}}),
            json!({"error": {
                "code": "Base.1.8.GeneralError",
                "@Message.ExtendedInfo": []
            }}),
            json!({"error": {
                "code": "Base.1.8.GeneralError",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]
            }}),
            json!({"error": {
                "code": "Base.1.8.InternalError",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.InternalError"}]
            }}),
            json!({
                "error": {"code": "Base.1.8.GeneralError", "message": "Failed."},
                "Success": {"code": "Base.1.10.Success", "message": "Done."}
            }),
            // A status object without a code cannot report a success.
            json!({"Success": "Base.1.10.Success"}),
            json!({
                "Success": "Base.1.10.Success",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.10.Created"}]
            }),
            json!({
                "error": {"message": "Nothing."},
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.NoOperation"}]
            }),
            json!({"error": {
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Created"}]
            }}),
            json!({
                "error": "Failed.",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]
            }),
            // A code that refers to its messages needs one that is a success.
            json!({"error": {
                "code": "iLO.0.10.ExtendedInfo",
                "message": "See @Message.ExtendedInfo for more information."
            }}),
            json!({"error": {
                "code": "iLO.0.10.ExtendedInfo",
                "@Message.ExtendedInfo": []
            }}),
            json!({"error": {
                "code": "iLO.0.10.ExtendedInfo",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.4.ActionParameterNotSupported"}]
            }}),
            // Messages must be an array of objects with a MessageId.
            json!({"@Message.ExtendedInfo": [{"Message": "Done."}]}),
            json!({"@Message.ExtendedInfo": [{"MessageId": 7}]}),
            json!({"@Message.ExtendedInfo": ["Base.1.8.Success"]}),
            json!({"@Message.ExtendedInfo": {"MessageId": "Base.1.8.Success"}}),
            json!({
                "Success": {"code": "Base.1.10.Success"},
                "@Message.ExtendedInfo": [{"Message": "Missing MessageId."}]
            }),
            json!({
                "Success": {"code": "Base.1.10.Success"},
                "@Message.ExtendedInfo": {"MessageId": "Base.1.8.ActionParameterNotSupported"}
            }),
        ] {
            assert_eq!(success_code(&body), None, "{body}");
            assert_eq!(strict_success_code(&body), None, "{body}");
        }
    }

    /// Verifies the success messages that BMCs send instead of `204 No
    /// Content`, as [`success_code`] and [`strict_success_code`] read them.
    #[test]
    fn vendor_success_messages_are_successes() {
        for (vendor, body, strict) in [
            (
                "bmcweb and AMI",
                json!({"@Message.ExtendedInfo": [{
                    "@odata.type": "#Message.v1_1_1.Message",
                    "Message": "The request completed successfully.",
                    "MessageArgs": [],
                    "MessageId": "Base.1.13.0.Success",
                    "MessageSeverity": "OK",
                    "Resolution": "None"
                }]}),
                Some(SuccessCode::Success),
            ),
            (
                "Supermicro",
                json!({"Success": {
                    "code": "Base.1.10.Success",
                    "message": "Successfully Completed Request."
                }}),
                Some(SuccessCode::Success),
            ),
            (
                "DSP0266",
                json!({"error": {
                    "code": "Base.1.8.Success",
                    "message": "Successfully Completed Request",
                    "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success", "Severity": "OK"}]
                }}),
                Some(SuccessCode::Success),
            ),
            (
                "HPE iLO",
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "message": "See @Message.ExtendedInfo for more information.",
                    "@Message.ExtendedInfo": [{"MessageId": "Base.1.4.Success"}]
                }}),
                Some(SuccessCode::Success),
            ),
            // A success with a note from another registry.
            (
                "Dell iDRAC",
                json!({"@Message.ExtendedInfo": [
                    {
                        "Message": "Successfully Completed Request",
                        "MessageArgs": [],
                        "MessageArgs@odata.count": 0,
                        "MessageId": "Base.1.12.Success",
                        "RelatedProperties": [],
                        "RelatedProperties@odata.count": 0,
                        "Resolution": "None",
                        "Severity": "OK"
                    },
                    {
                        "Message": "The operation successfully completed.",
                        "MessageArgs": [],
                        "MessageArgs@odata.count": 0,
                        "MessageId": "IDRAC.2.9.SYS413",
                        "RelatedProperties": [],
                        "RelatedProperties@odata.count": 0,
                        "Resolution": "No response action is required.",
                        "Severity": "Informational"
                    }
                ]}),
                None,
            ),
        ] {
            assert_eq!(success_code(&body), Some(SuccessCode::Success), "{vendor}");
            assert_eq!(strict_success_code(&body), strict, "{vendor}");
        }
    }

    /// Verifies that [`strict_success_code`] reports a success that names its
    /// outcome and reports nothing else, as [`success_code`] does.
    #[test]
    fn a_strict_success_names_its_outcome_and_nothing_else() {
        for (body, code) in [
            (
                json!({"@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]}),
                SuccessCode::Success,
            ),
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.Success",
                    "MessageSeverity": "OK",
                    "Severity": "OK"
                }]}),
                SuccessCode::Success,
            ),
            (
                json!({"Success": {"code": "Base.1.10.Success"}}),
                SuccessCode::Success,
            ),
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.Success",
                    "MessageArgs": [1]
                }]}),
                SuccessCode::Success,
            ),
            (
                json!({"@Message.ExtendedInfo": [
                    {"MessageId": "Base.1.8.Success"},
                    {"MessageId": "Base.1.8.Created"}
                ]}),
                SuccessCode::Created,
            ),
            (
                json!({"error": {"code": "Base.1.8.Created", "message": "Created."}}),
                SuccessCode::Created,
            ),
            // NoOperation is a Warning in the Base registry.
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.NoOperation",
                    "MessageSeverity": "Warning"
                }]}),
                SuccessCode::NoOperation,
            ),
            (
                json!({"Success": {"code": "Base.1.10.NoOperation", "message": "Nothing."}}),
                SuccessCode::NoOperation,
            ),
        ] {
            assert_eq!(success_code(&body), Some(code), "{body}");
            assert_eq!(strict_success_code(&body), Some(code), "{body}");
        }
    }

    /// Verifies that [`strict_success_code`] does not report a success that
    /// [`success_code`] reads with a qualification.
    #[test]
    fn a_qualified_success_is_not_a_strict_success() {
        for (body, code) in [
            // Nothing names the outcome.
            (json!({"@Message.ExtendedInfo": []}), SuccessCode::Success),
            (
                json!({"@Message.ExtendedInfo": [{"MessageId": "IDRAC.2.9.SYS413"}]}),
                SuccessCode::Success,
            ),
            // A warning, or a severity worse than the registry gives.
            (
                json!({"@Message.ExtendedInfo": [
                    {"MessageId": "Base.1.8.Success"},
                    {
                        "MessageId": "Base.1.8.ActionParameterNotSupported",
                        "MessageSeverity": "Warning"
                    }
                ]}),
                SuccessCode::Success,
            ),
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.Success",
                    "MessageSeverity": "Warning"
                }]}),
                SuccessCode::Success,
            ),
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.Created",
                    "Severity": "Warning"
                }]}),
                SuccessCode::Created,
            ),
            (
                json!({"@Message.ExtendedInfo": [{
                    "MessageId": "Base.1.8.Success",
                    "Severity": "Informational"
                }]}),
                SuccessCode::Success,
            ),
            (
                json!({
                    "Success": {"code": "Base.1.10.Success"},
                    "@Message.ExtendedInfo": [{
                        "MessageId": "Base.1.8.ActionParameterNotSupported",
                        "MessageSeverity": "Warning"
                    }]
                }),
                SuccessCode::Success,
            ),
            (
                json!({"error": {
                    "code": "Base.1.8.Success",
                    "@Message.ExtendedInfo": [{
                        "MessageId": "Base.1.8.ActionParameterNotSupported",
                        "MessageSeverity": "Warning"
                    }]
                }}),
                SuccessCode::Success,
            ),
            (
                json!({"error": {
                    "code": "iLO.0.10.ExtendedInfo",
                    "@Message.ExtendedInfo": [
                        {"MessageId": "Base.1.4.Success"},
                        {"MessageId": "Base.1.4.PropertyValueModified"}
                    ]
                }}),
                SuccessCode::Success,
            ),
            // Part of the request did nothing.
            (
                json!({"@Message.ExtendedInfo": [
                    {"MessageId": "Base.1.8.Success"},
                    {"MessageId": "Base.1.8.NoOperation"}
                ]}),
                SuccessCode::Success,
            ),
            (
                json!({
                    "error": {"code": "Base.1.8.NoOperation", "message": "Nothing."},
                    "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]
                }),
                SuccessCode::NoOperation,
            ),
        ] {
            assert_eq!(success_code(&body), Some(code), "{body}");
            assert_eq!(strict_success_code(&body), None, "{body}");
        }
    }

    /// Verifies that a `200 OK` success message decodes as [`ModificationResponse::Message`].
    #[test]
    fn a_success_message_is_a_message_response() -> Result<(), Box<dyn StdError>> {
        for body in [
            json!({"error": {"code": "Base.1.8.Success", "message": "Done."}}),
            json!({"Success": {"code": "Base.1.10.Success", "message": "Done."}}),
            json!({"Success": {"code": "Base.1.10.Success"}}),
            json!({
                "Success": {"code": "Base.1.10.Success"},
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.10.Success"}]
            }),
            json!({"error": {
                "code": "iLO.0.10.ExtendedInfo",
                "message": "See @Message.ExtendedInfo for more information.",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.4.Success"}]
            }}),
            json!({"@Message.ExtendedInfo": [{"MessageId": "Base.1.13.0.Success"}]}),
            // NoOperation and warnings are successes too; strict_success_code
            // tells them apart on the preserved ModificationResponse::Message body.
            json!({"Success": {"code": "Base.1.10.NoOperation", "message": "Nothing."}}),
            json!({"@Message.ExtendedInfo": [
                {"MessageId": "Base.1.8.Success"},
                {"MessageId": "Base.1.8.PropertyValueModified", "MessageSeverity": "Warning"}
            ]}),
        ] {
            let response = decode::<()>(StatusCode::OK, &HeaderMap::new(), &body)?;
            assert!(
                matches!(response, ModificationResponse::Message(ref message) if message == &body),
                "{}: {:?}",
                body,
                response
            );
        }
        Ok(())
    }

    /// Verifies that non-success message envelopes return [`ModificationDecodeError::InvalidResponse`]
    /// with the body text, while non-message payloads and malformed JSON return parse errors.
    #[test]
    fn a_body_that_is_neither_the_result_nor_a_success_message_fails() {
        for body in [
            json!({"error": {"code": "Base.1.8.GeneralError", "message": "Failed."}}),
            json!({"error": {"code": "Base.1.8.GeneralError", "@Message.ExtendedInfo": []}}),
            json!({"error": {
                "code": "Base.1.8.GeneralError",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.8.Success"}]
            }}),
            json!({"error": {
                "code": "iLO.0.10.ExtendedInfo",
                "@Message.ExtendedInfo": [{"MessageId": "Base.1.4.ActionParameterNotSupported"}]
            }}),
            json!({"Success": {"code": "Base.1.10.GeneralError", "message": "Failed."}}),
            json!({"@Message.ExtendedInfo": [{
                "MessageId": "Base.1.18.1.PropertyValueNotInList",
                "Message": "The value 'X' for the property Y is not in the list of acceptable values.",
                "MessageSeverity": "Critical"
            }]}),
        ] {
            let error =
                decode::<()>(StatusCode::OK, &HeaderMap::new(), &body).expect_err("not a success");
            assert!(
                matches!(
                    error,
                    ModificationDecodeError::InvalidResponse {
                        status: StatusCode::OK,
                        ref text,
                        ..
                    } if text == &body.to_string()
                ),
                "{}: {}",
                body,
                error
            );
            assert_eq!(
                error.error_class(),
                BmcErrorClass::HttpResponse { status: 200 }
            );
        }

        let body = json!({"Result": "Accepted"});
        let error = decode::<()>(StatusCode::OK, &HeaderMap::new(), &body)
            .expect_err("neither T nor a message");
        assert!(
            matches!(error, ModificationDecodeError::Decode(_)),
            "{}: {}",
            body,
            error
        );
        assert_eq!(error.error_class(), BmcErrorClass::ResponseParse);

        let error = decode_modification::<()>(&url(), StatusCode::OK, &HeaderMap::new(), b"{")
            .expect_err("not JSON");
        assert!(matches!(error, ModificationDecodeError::Json(_)));
    }

    /// Verifies that a success message accompanied by a `Location` header resolves to a reference.
    #[test]
    fn a_success_message_with_a_location_is_that_resource() -> Result<(), Box<dyn StdError>> {
        let location = headers(&[(header::LOCATION, "/redfish/v1/TaskService/Tasks/42")]);
        let body = json!({"@Message.ExtendedInfo": [{"MessageId": "Base.1.12.Success"}]});

        let response = decode::<Reference>(StatusCode::OK, &location, &body)?;
        let ModificationResponse::Entity(reference) = response else {
            return Err("expected the resource the Location names".into());
        };
        assert_eq!(reference.odata_id, "/redfish/v1/TaskService/Tasks/42");

        // A result type that is not a reference ignores the Location and keeps the message body.
        let response = decode::<()>(StatusCode::OK, &location, &body)?;
        assert!(matches!(response, ModificationResponse::Message(ref message) if message == &body));
        Ok(())
    }

    /// Verifies header-driven decoding for `204 No Content`, `202 Accepted`, and empty `201 Created`.
    #[test]
    fn statuses_without_a_result_decode_by_their_headers() -> Result<(), Box<dyn StdError>> {
        let response =
            decode_modification::<()>(&url(), StatusCode::NO_CONTENT, &HeaderMap::new(), b"")?;
        assert!(matches!(response, ModificationResponse::Empty));

        let accepted = headers(&[
            (header::LOCATION, "/redfish/v1/TaskService/Tasks/7"),
            (header::RETRY_AFTER, "5"),
        ]);
        let ModificationResponse::Task(task) =
            decode_modification::<()>(&url(), StatusCode::ACCEPTED, &accepted, b"")?
        else {
            return Err("202 is a task".into());
        };
        assert_eq!(
            task.location.0.to_string(),
            "/redfish/v1/TaskService/Tasks/7"
        );
        assert_eq!(task.retry_after, Some(Duration::from_secs(5)));
        assert_eq!(task.task, None);

        let task_body = json!({
            "@odata.id": "/redfish/v1/TaskService/Tasks/7",
            "@odata.type": "#Task.v1_4_3.Task",
        });
        let ModificationResponse::Task(task_with_body) = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &accepted,
            task_body.to_string().as_bytes(),
        )?
        else {
            return Err("202 with Task body is a task".into());
        };
        assert_eq!(
            task_with_body
                .task
                .as_ref()
                .map(ToString::to_string)
                .as_deref(),
            Some("/redfish/v1/TaskService/Tasks/7")
        );

        let ModificationResponse::Task(task_with_unparsable_body) = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &accepted,
            b"not valid json {",
        )?
        else {
            return Err("202 with unparsable body and valid Location is still a task".into());
        };
        assert_eq!(
            task_with_unparsable_body.location.0.to_string(),
            "/redfish/v1/TaskService/Tasks/7"
        );
        assert_eq!(task_with_unparsable_body.task, None);

        let error = decode_modification::<()>(&url(), StatusCode::ACCEPTED, &HeaderMap::new(), b"")
            .expect_err("202 without Location");
        assert!(matches!(
            error,
            ModificationDecodeError::InvalidResponse { ref text, .. }
                if text == "202 Accepted without Location header"
        ));

        let created = headers(&[(header::LOCATION, "Tasks/8")]);
        let ModificationResponse::Entity(reference) =
            decode_modification::<Reference>(&url(), StatusCode::CREATED, &created, b"")?
        else {
            return Err("an empty 201 with a Location is that resource".into());
        };
        assert_eq!(reference.odata_id, "/redfish/v1/Systems/1/Actions/Tasks/8");
        Ok(())
    }

    /// Verifies that a `202 Accepted` is polled at its task monitor, and at the
    /// `Task` its body represents when it names no `Location`.
    #[test]
    fn an_accepted_response_is_polled_at_its_monitor_or_its_task() -> Result<(), Box<dyn StdError>>
    {
        let task_body = |id: &str| {
            json!({
                "@odata.id": id,
                "@odata.type": "#Task.v1_4_3.Task",
                "Id": "7",
                "TaskState": "Running"
            })
            .to_string()
        };

        let monitor = headers(&[
            (header::LOCATION, "/redfish/v1/TaskService/TaskMonitors/7"),
            (header::RETRY_AFTER, "3"),
        ]);
        let ModificationResponse::Task(task) = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &monitor,
            task_body("https://bmc.example/redfish/v1/TaskService/Tasks/7?x=1").as_bytes(),
        )?
        else {
            return Err("a 202 with a Location is a task".into());
        };
        assert_eq!(
            task.location.0.to_string(),
            "/redfish/v1/TaskService/TaskMonitors/7"
        );
        assert_eq!(task.retry_after, Some(Duration::from_secs(3)));
        assert_eq!(
            task.task.as_ref().map(ToString::to_string).as_deref(),
            Some("/redfish/v1/TaskService/Tasks/7?x=1")
        );

        let ModificationResponse::Task(task_with_cross_origin_body) = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &monitor,
            task_body("https://other.example/redfish/v1/TaskService/Tasks/7").as_bytes(),
        )?
        else {
            return Err("a 202 with a Location ignores a cross-origin Task body".into());
        };
        assert_eq!(
            task_with_cross_origin_body.location.0.to_string(),
            "/redfish/v1/TaskService/TaskMonitors/7"
        );
        assert_eq!(task_with_cross_origin_body.task, None);

        let retry = headers(&[(header::RETRY_AFTER, "3")]);
        let ModificationResponse::Task(task) = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &retry,
            task_body("/redfish/v1/TaskService/Tasks/7").as_bytes(),
        )?
        else {
            return Err("a 202 with a Task body is a task".into());
        };
        assert_eq!(
            task.location.0.to_string(),
            "/redfish/v1/TaskService/Tasks/7"
        );
        assert_eq!(task.retry_after, Some(Duration::from_secs(3)));
        assert_eq!(
            task.task.as_ref().map(ToString::to_string).as_deref(),
            Some("/redfish/v1/TaskService/Tasks/7")
        );

        let error = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &HeaderMap::new(),
            task_body("https://other.example/redfish/v1/TaskService/Tasks/7").as_bytes(),
        )
        .expect_err("a Task of another origin");
        assert!(matches!(
            error,
            ModificationDecodeError::InvalidResponse { ref text, .. }
                if text == "202 Accepted without Location header"
        ));

        // Without a Task to poll, as Supermicro answers a staged change, a 202
        // without Location stays an error.
        let error = decode_modification::<()>(
            &url(),
            StatusCode::ACCEPTED,
            &HeaderMap::new(),
            json!({
                "@odata.id": "/redfish/v1/Systems/1/Bios/Settings",
                "@odata.type": "#Bios.v1_2_0.Bios"
            })
            .to_string()
            .as_bytes(),
        )
        .expect_err("a 202 with neither a Location nor a Task");
        assert!(matches!(
            error,
            ModificationDecodeError::InvalidResponse { ref text, status, .. }
                if text == "202 Accepted without Location header"
                    && status == StatusCode::ACCEPTED
        ));
        assert_eq!(
            error.error_class(),
            BmcErrorClass::HttpResponse { status: 202 }
        );
        Ok(())
    }

    /// Verifies that unsupported `2xx` and non-`2xx` statuses return [`ModificationDecodeError::InvalidResponse`].
    #[test]
    fn unexpected_and_unsuccessful_statuses_are_invalid_responses() {
        let error =
            decode_modification::<()>(&url(), StatusCode::PARTIAL_CONTENT, &HeaderMap::new(), b"")
                .expect_err("206");
        assert!(matches!(
            error,
            ModificationDecodeError::InvalidResponse { ref text, .. }
                if text == "Unexpected successful status code: 206 Partial Content"
        ));

        let error = decode_modification::<()>(
            &url(),
            StatusCode::BAD_REQUEST,
            &HeaderMap::new(),
            br#"{"error":{"code":"Base.1.8.ActionParameterUnknown","message":"Bad."}}"#,
        )
        .expect_err("400");
        assert!(matches!(
            error,
            ModificationDecodeError::InvalidResponse { ref text, .. }
                if text.contains("ActionParameterUnknown")
        ));
        assert_eq!(
            error.error_class(),
            BmcErrorClass::HttpResponse { status: 400 }
        );
    }

    /// Verifies that the HTTP `ETag` header is injected into a returned resource with `@odata.id`.
    #[test]
    fn the_etag_header_is_the_etag_of_a_returned_resource() -> Result<(), Box<dyn StdError>> {
        let etag = headers(&[(header::ETAG, "\"7\"")]);
        let ModificationResponse::Entity(resource) = decode::<Resource>(
            StatusCode::OK,
            &etag,
            &json!({"@odata.id": "/redfish/v1/AccountService"}),
        )?
        else {
            return Err("expected the resource".into());
        };
        assert_eq!(resource.odata_id, "/redfish/v1/AccountService");
        assert_eq!(resource.odata_etag.as_deref(), Some("\"7\""));
        Ok(())
    }

    /// Verifies [`decode_poll`] handling of `202 Accepted` without `Location`, `204` with `Location`, and `200`.
    #[test]
    fn a_pending_poll_without_a_location_stays_at_the_polled_uri() -> Result<(), Box<dyn StdError>>
    {
        let polled = Url::parse("https://bmc.example/redfish/v1/TaskService/TaskMonitors/1?x=y")?;
        let ModificationResponse::Task(task) =
            decode_poll::<()>(&polled, StatusCode::ACCEPTED, &HeaderMap::new(), b"")?
        else {
            return Err("202 is still running".into());
        };
        assert_eq!(
            task.location.0.to_string(),
            "/redfish/v1/TaskService/TaskMonitors/1?x=y"
        );

        let polled_with_location =
            headers(&[(header::LOCATION, "/redfish/v1/TaskService/TaskMonitors/2")]);
        let ModificationResponse::Task(task_with_unparsable_body) = decode_poll::<()>(
            &polled,
            StatusCode::ACCEPTED,
            &polled_with_location,
            b"not valid json {",
        )?
        else {
            return Err("202 poll with unparsable body and valid Location is still a task".into());
        };
        assert_eq!(
            task_with_unparsable_body.location.0.to_string(),
            "/redfish/v1/TaskService/TaskMonitors/2"
        );
        assert_eq!(task_with_unparsable_body.task, None);

        let created = headers(&[(header::LOCATION, "/redfish/v1/Systems/1")]);
        let ModificationResponse::Entity(reference) =
            decode_poll::<Reference>(&polled, StatusCode::NO_CONTENT, &created, b"")?
        else {
            return Err("a 204 with a Location is that resource".into());
        };
        assert_eq!(reference.odata_id, "/redfish/v1/Systems/1");

        let response = decode_poll::<()>(
            &polled,
            StatusCode::OK,
            &HeaderMap::new(),
            br#"{"Success":{"code":"Base.1.10.Success","message":"Done."}}"#,
        )?;
        assert!(matches!(response, ModificationResponse::Message(_)));

        let response = decode_poll::<()>(
            &polled,
            StatusCode::OK,
            &HeaderMap::new(),
            br#"{"Success":{"code":"Base.1.10.Success"},"@Message.ExtendedInfo":[{"MessageId":"Base.1.10.Success"}]}"#,
        )?;
        assert!(matches!(response, ModificationResponse::Message(_)));
        Ok(())
    }

    /// Verifies [`location_from_headers`] resolution and same-origin validation across URI forms.
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
            // Credentials and a fragment never reach the returned path.
            TestCase(
                "https://user:secret@bmc.example/redfish/v1/TaskService/Tasks/42#done",
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

            let result = location_from_headers(&headers, &response_url, StatusCode::CREATED);

            match (result, expected) {
                (Ok(Some(location)), Ok(expected)) => {
                    assert_eq!(location.to_string(), expected, "{raw}");
                }
                (Err(ModificationDecodeError::InvalidResponse { text, .. }), Err(expected)) => {
                    assert_eq!(text, expected, "{raw}");
                }
                _ => return Err(format!("{raw}: unexpected Location result").into()),
            }
        }

        Ok(())
    }
}
