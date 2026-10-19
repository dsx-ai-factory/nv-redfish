// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Repairs the caller supplies: [`UserRule`]s, applied by a [`RulesBmc`]
//! to every document read through the transport it wraps, after any
//! repair the read asks for.
//!
//! The rules are the transport's for its whole life. A [`RulesBmc`] owns
//! the transport it wraps, so every read through it is repaired with these
//! rules and no others: new rules take a new transport.

use std::fmt;
use std::sync::Arc;

use futures_util::StreamExt as _;
use nv_redfish_core::query::ExpandQuery;
use nv_redfish_core::repair::decode;
use nv_redfish_core::Action;
use nv_redfish_core::Bmc;
use nv_redfish_core::BoxTryStream;
use nv_redfish_core::DecodeError;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
use nv_redfish_core::FilterQuery;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::MultipartUpdateRequest;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::Repair;
use nv_redfish_core::RepairError;
use nv_redfish_core::SessionCreateResponse;
use nv_redfish_core::StreamEvent;
use nv_redfish_core::UploadReader;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Map;
use serde_json::Value;

use crate::rules::walk;
use crate::rules::Fix;
use crate::rules::TypeName;

/// A caller's document rewrite.
pub type UserFix = Arc<dyn Fn(Value) -> Value + Send + Sync>;

/// Which documents a [`UserRule`] applies to.
#[derive(Debug)]
#[non_exhaustive]
pub enum Match {
    /// Documents of this type, in any version. A resource type is named by
    /// its schema, `"ManagerNetworkProtocol"` for
    /// `#ManagerNetworkProtocol.v1_9_0.ManagerNetworkProtocol`; any type by
    /// its schema and name, `"Chassis.Links"` for `#Chassis.v1_22_0.Links`,
    /// which `"Chassis"` does not match.
    Type(String),
    /// Documents whose `@odata.id` matches this pattern.
    Id(IdPattern),
}

impl Match {
    fn applies(&self, type_name: Option<TypeName<'_>>, odata_id: Option<&str>) -> bool {
        match self {
            Self::Type(named) => type_name.is_some_and(|type_name| type_name.is_named(named)),
            Self::Id(pattern) => odata_id.is_some_and(|id| pattern.matches(id)),
        }
    }
}

/// An `@odata.id` pattern: path segments compared one by one, where a `*`
/// segment matches any single segment.
///
/// `/redfish/v1/Managers/*/NetworkProtocol` matches
/// `/redfish/v1/Managers/BMC/NetworkProtocol` but not
/// `/redfish/v1/Managers/BMC/Extra/NetworkProtocol`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdPattern(Vec<String>);

impl IdPattern {
    /// The pattern `pattern`.
    #[must_use]
    pub fn new(pattern: &str) -> Self {
        Self(pattern.split('/').map(str::to_owned).collect())
    }

    /// Whether `odata_id` matches.
    #[must_use]
    pub fn matches(&self, odata_id: &str) -> bool {
        let mut segments = odata_id.split('/');
        self.0.iter().all(|expected| {
            segments
                .next()
                .is_some_and(|s| expected == "*" || expected == s)
        }) && segments.next().is_none()
    }
}

/// A named repair for the documents [`Match`] selects.
pub struct UserRule {
    name: String,
    matches: Match,
    fix: UserFix,
}

impl UserRule {
    /// The rule `name` that applies `fix` to the documents `matches`
    /// selects.
    pub fn new(
        name: impl Into<String>,
        matches: Match,
        fix: impl Fn(Value) -> Value + Send + Sync + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            matches,
            fix: Arc::new(fix),
        }
    }

    /// The rule's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Debug for UserRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UserRule")
            .field("name", &self.name)
            .field("matches", &self.matches)
            .finish_non_exhaustive()
    }
}

/// The document's `@odata.id`, unless the object is a bare reference: a
/// link names a document, it is not one.
fn document_id_of(object: &Map<String, Value>) -> Option<&str> {
    let id = object.get("@odata.id")?.as_str()?;
    (object.len() > 1).then_some(id)
}

/// The caller's rules as one repair: each rule, in the order given, on
/// every document it matches, at any depth.
#[derive(Clone)]
struct Rules(Arc<[UserRule]>);

impl Repair for Rules {
    fn repair(&self, document: &mut Value) {
        if self.0.is_empty() {
            return;
        }
        walk(document, &|object| {
            let type_name = TypeName::of(object);
            let odata_id = document_id_of(object);
            self.0
                .iter()
                .filter(|rule| rule.matches.applies(type_name, odata_id))
                .map(|rule| -> Fix<'_> { &*rule.fix })
                .collect()
        });
    }
}

/// The repair a read asks for, then the caller's rules.
struct Then<'a, R>(&'a R, &'a Rules);

impl<R: Repair> Repair for Then<'_, R> {
    fn repair(&self, document: &mut Value) {
        self.0.repair(document);
        self.1.repair(document);
    }
}

/// A transport that repairs every document read through it with the
/// caller's rules.
///
/// The rules run after any repair the read asks for: the platform's, when a
/// [`CompatBmc`](crate::CompatBmc) reads through it. A document the layer
/// above repairs on its own afterwards, such as an event stream payload or
/// the service root, gets the rules first.
///
/// The rules are the transport's for its whole life. A `RulesBmc` owns the
/// transport it wraps, so every read through it is repaired with these
/// rules and no others: new rules take a new transport.
///
/// An event stream payload that does not deserialize after the rules is
/// one error in the stream, not its end. Writes go through as they are.
pub struct RulesBmc<B> {
    inner: B,
    rules: Rules,
}

impl<B> RulesBmc<B> {
    /// `inner`, repairing what is read through it with `rules`, applied in
    /// the order given.
    #[must_use]
    pub fn new(inner: B, rules: Vec<UserRule>) -> Self {
        Self {
            inner,
            rules: Rules(rules.into()),
        }
    }

    /// The rules, in the order they apply.
    #[must_use]
    pub fn rules(&self) -> &[UserRule] {
        &self.rules.0
    }

    /// The transport underneath, for what it offers beyond [`Bmc`], such
    /// as new credentials. Do not read through it: such a read is not
    /// repaired, and a caching transport serves what it read to the
    /// repaired reads of the same resource that follow, until the resource
    /// changes.
    #[must_use]
    pub const fn inner(&self) -> &B {
        &self.inner
    }
}

/// A failed read through the transport, with a document that did not
/// deserialize reported the way the transport reports one.
fn flatten<E: From<DecodeError>>(error: RepairError<E>) -> E {
    match error {
        RepairError::Transport(error) => error,
        RepairError::Decode(error) => error.into(),
    }
}

impl<B: Bmc> Bmc for RulesBmc<B>
where
    B::Error: From<DecodeError>,
{
    type Error = B::Error;

    async fn expand<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
    ) -> Result<Arc<T>, Self::Error> {
        self.inner
            .expand_repaired(id, query, &self.rules)
            .await
            .map_err(flatten)
    }

    async fn get<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        self.inner
            .get_repaired(id, &self.rules)
            .await
            .map_err(flatten)
    }

    async fn filter<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
    ) -> Result<Arc<T>, Self::Error> {
        self.inner
            .filter_repaired(id, query, &self.rules)
            .await
            .map_err(flatten)
    }

    async fn get_repaired<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        repair: &impl Repair,
    ) -> Result<Arc<T>, RepairError<Self::Error>> {
        self.inner
            .get_repaired(id, &Then(repair, &self.rules))
            .await
    }

    async fn expand_repaired<T: Expandable>(
        &self,
        id: &ODataId,
        query: ExpandQuery,
        repair: &impl Repair,
    ) -> Result<Arc<T>, RepairError<Self::Error>> {
        self.inner
            .expand_repaired(id, query, &Then(repair, &self.rules))
            .await
    }

    async fn filter_repaired<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static>(
        &self,
        id: &ODataId,
        query: FilterQuery,
        repair: &impl Repair,
    ) -> Result<Arc<T>, RepairError<Self::Error>> {
        self.inner
            .filter_repaired(id, query, &Then(repair, &self.rules))
            .await
    }

    async fn create<V: Send + Sync + Serialize, R: Send + Sync + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner.create(id, query).await
    }

    async fn create_session<
        V: Send + Sync + Serialize,
        R: Send + Sync + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        query: &V,
    ) -> Result<SessionCreateResponse<R>, Self::Error> {
        self.inner.create_session(id, query).await
    }

    async fn update<
        V: Sync + Send + Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        etag: Option<&ODataETag>,
        update: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner.update(id, etag, update).await
    }

    async fn delete<R: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner.delete(id).await
    }

    async fn action<
        T: Send + Sync + Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        action: &Action<T, R>,
        params: &T,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner.action(action, params).await
    }

    async fn poll<R: Send + Sync + Sized + for<'de> Deserialize<'de>>(
        &self,
        uri: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        self.inner.poll(uri).await
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
        self.inner.multipart_update(uri, request).await
    }

    nv_redfish_core::if_update_service_deprecated! {
        async fn http_push_uri_update<U, R>(
            &self,
            uri: &str,
            request: nv_redfish_core::HttpPushUriUpdateRequest<U>,
        ) -> Result<ModificationResponse<R>, Self::Error>
        where
            U: UploadReader,
            R: Send + Sync + for<'de> Deserialize<'de>,
        {
            self.inner.http_push_uri_update(uri, request).await
        }
    }

    async fn stream<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
    ) -> Result<BoxTryStream<T, Self::Error>, Self::Error> {
        let stream = self.inner.stream::<Value>(uri).await?;
        let rules = self.rules.clone();
        let mapped: BoxTryStream<T, Self::Error> = Box::pin(stream.map(move |item| {
            let mut payload = item?;
            rules.repair(&mut payload);
            decode(payload).map_err(Into::into)
        }));
        Ok(mapped)
    }

    async fn stream_events<T: Sized + for<'de> Deserialize<'de> + Send + 'static>(
        &self,
        uri: &str,
        last_event_id: Option<&str>,
    ) -> Result<BoxTryStream<StreamEvent<T>, Self::Error>, Self::Error> {
        let stream = self
            .inner
            .stream_events::<Value>(uri, last_event_id)
            .await?;
        let rules = self.rules.clone();
        let mapped: BoxTryStream<StreamEvent<T>, Self::Error> = Box::pin(stream.map(move |item| {
            let StreamEvent {
                last_event_id,
                data: mut payload,
            } = item?;
            rules.repair(&mut payload);
            decode(payload)
                .map(|data| StreamEvent {
                    last_event_id,
                    data,
                })
                .map_err(Into::into)
        }));
        Ok(mapped)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::rules::enabled;
    use crate::BmcQuirks;
    use crate::RootEvidence;

    fn rules(rules: Vec<UserRule>) -> Rules {
        Rules(rules.into())
    }

    #[test]
    fn an_id_pattern_matches_segment_by_segment() {
        let pattern = IdPattern::new("/redfish/v1/Managers/*/NetworkProtocol");
        assert!(pattern.matches("/redfish/v1/Managers/BMC/NetworkProtocol"));
        assert!(!pattern.matches("/redfish/v1/Managers/BMC/Extra/NetworkProtocol"));
        assert!(!pattern.matches("/redfish/v1/Managers/BMC"));
        assert!(!pattern.matches("/redfish/v1/Managers/BMC/NetworkProtocol/More"));
    }

    #[test]
    fn a_rule_reaches_the_documents_it_matches_and_skips_references() {
        let rules = rules(vec![
            UserRule::new("name", Match::Type("Chassis".into()), |mut v| {
                v["Name"] = json!("named");
                v
            }),
            UserRule::new(
                "tag",
                Match::Id(IdPattern::new("/redfish/v1/Chassis/*")),
                |mut v| {
                    v["Tagged"] = json!(true);
                    v
                },
            ),
        ]);
        let links = json!({
            "@odata.type": "#Chassis.v1_22_0.Links",
            "Contains": [{ "@odata.id": "/redfish/v1/Chassis/2U" }],
        });
        let mut document = json!({
            "@odata.id": "/redfish/v1/Chassis/1U",
            "@odata.type": "#Chassis.v1_22_0.Chassis",
            "Links": links,
        });

        rules.repair(&mut document);
        assert_eq!(document["Name"], "named");
        assert_eq!(document["Tagged"], true);
        // Another type of the schema, and a link to another chassis, are
        // left as they came.
        assert_eq!(document["Links"], links);
    }

    #[test]
    fn the_callers_rules_follow_the_repair_the_read_asks_for() {
        let viking = enabled(&BmcQuirks::classify(&RootEvidence::from_root(&json!({
            "Vendor": "AMI",
            "RedfishVersion": "1.11.0",
        }))));
        let rules = rules(vec![UserRule::new(
            "rename",
            Match::Type("Chassis".into()),
            |mut v| {
                v["Name"] = json!(format!("{} (renamed)", v["Name"].as_str().unwrap_or("")));
                v
            },
        )]);
        let mut chassis = json!({ "@odata.type": "#Chassis.v1_2_0.Chassis", "Id": "1U" });

        // The platform names the chassis; the caller's rule renames it.
        Then(&viking, &rules).repair(&mut chassis);
        assert_eq!(chassis["Name"], "Unnamed chassis (renamed)");
    }
}
