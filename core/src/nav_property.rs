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

//! Navigation property wrapper for generated types
//!
//! Represents Redfish/OData navigation properties which may appear either as
//! a reference (only `@odata.id`) or as an expanded object. Generated code wraps
//! navigation properties in [`NavProperty<T>`], allowing code to work uniformly
//! with both forms and resolve references on demand.
//!
//! - Reference form: `{ "@odata.id": "/redfish/v1/Chassis/1/Thermal" }`
//! - Expanded form: full object payload for `T` (includes `@odata.id` and fields)
//!
//! Key points
//! - [`NavProperty<T>::id`] is always available (delegates to inner entity for expanded form).
//! - [`NavProperty<T>::get`] returns `Arc<T>`; if already expanded, it clones the `Arc` without I/O.
//! - [`EntityTypeRef::etag`] is `None` for reference form.
//!
//! References:
//! - DMTF Redfish Specification DSP0266 — `https://www.dmtf.org/standards/redfish`
//! - OASIS OData 4.01 — navigation properties in CSDL
//!

use crate::Bmc;
use crate::Creatable;
use crate::Deletable;
use crate::EntityTypeRef;
use crate::Expandable;
use crate::FilterQuery;
use crate::ODataETag;
use crate::ODataId;
use crate::Updatable;
use serde::de;
use serde::de::Deserializer;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use std::sync::Arc;

/// Reference variant of the navigation property (only `@odata.id`
/// property is specified).
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
}

impl<T: EntityTypeRef> From<&NavProperty<T>> for Reference {
    fn from(v: &NavProperty<T>) -> Self {
        Self {
            odata_id: v.id().clone(),
        }
    }
}

impl From<&Self> for Reference {
    fn from(v: &Self) -> Self {
        Self {
            odata_id: v.odata_id.clone(),
        }
    }
}

impl From<&ReferenceLeaf> for Reference {
    fn from(v: &ReferenceLeaf) -> Self {
        Self {
            odata_id: v.odata_id.clone(),
        }
    }
}

/// `ReferenceLeaf` is special type that is used for navigation
/// properties that if corresponding `EntityType` was not compiled to
/// the tree.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReferenceLeaf {
    /// `OData` identifier for of the property.
    #[serde(rename = "@odata.id")]
    pub odata_id: ODataId,
}

/// Container struct for the expanded property variant.
#[derive(Debug)]
pub struct Expanded<T>(Arc<T>);

/// Deserializer that wraps the expanded property value into an `Arc`.
impl<'de, T> Deserialize<'de> for Expanded<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        T::deserialize(deserializer).map(Arc::new).map(Expanded)
    }
}

/// Navigation property variants. All navigation properties in
/// generated code are wrapped with this type.
#[derive(Debug)]
pub enum NavProperty<T: EntityTypeRef> {
    /// Expanded property variant (content included in the
    /// response).
    Expanded(Expanded<T>),
    /// Reference variant (only `@odata.id` is included in the
    /// response).
    Reference(Reference),
}

impl<'de, T> Deserialize<'de> for NavProperty<T>
where
    T: EntityTypeRef + for<'dt> Deserialize<'dt>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        let is_reference = value
            .as_object()
            .is_some_and(|obj| obj.len() == 1 && obj.contains_key("@odata.id"));

        if is_reference {
            let reference = serde_json::from_value::<Reference>(value)
                .map_err(|err| de::Error::custom(err.to_string()))?;
            Ok(Self::Reference(reference))
        } else {
            // Non-reference payloads are always parsed as expanded `T`.
            let expanded = serde_json::from_value::<T>(value)
                .map_err(|err| de::Error::custom(err.to_string()))?;
            Ok(Self::Expanded(Expanded(Arc::new(expanded))))
        }
    }
}

impl<T> Serialize for NavProperty<T>
where
    T: EntityTypeRef + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Expanded(expanded) => expanded.0.as_ref().serialize(serializer),
            Self::Reference(reference) => reference.serialize(serializer),
        }
    }
}

impl<T: EntityTypeRef> EntityTypeRef for NavProperty<T> {
    fn odata_id(&self) -> &ODataId {
        match self {
            Self::Expanded(v) => v.0.odata_id(),
            Self::Reference(r) => &r.odata_id,
        }
    }

    fn etag(&self) -> Option<&ODataETag> {
        match self {
            Self::Expanded(v) => v.0.etag(),
            Self::Reference(_) => None,
        }
    }
}

impl<C, R, T: Creatable<C, R>> Creatable<C, R> for NavProperty<T>
where
    C: Send + Sync + Serialize,
    R: Send + Sync + for<'de> Deserialize<'de>,
{
}
impl<U, T: Updatable<U>> Updatable<U> for NavProperty<T> where U: Sync + Send + Sized + Serialize {}
impl<T: Deletable> Deletable for NavProperty<T> {}
impl<T: Expandable> Expandable for NavProperty<T> {}

impl<T: EntityTypeRef> NavProperty<T> {
    /// Create a navigation property with a reference using the `OData`
    /// identifier.
    #[must_use]
    pub const fn new_reference(odata_id: ODataId) -> Self {
        Self::Reference(Reference { odata_id })
    }

    /// Convert property to reference regardless expanded it or not.
    #[must_use]
    pub fn to_reference(self) -> Self {
        match self {
            Self::Reference(_) => self,
            Self::Expanded(_) => Self::new_reference(self.id().clone()),
        }
    }

    /// Downcast to descendant type `D`.
    #[must_use]
    pub fn downcast<D: EntityTypeRef>(&self) -> NavProperty<D> {
        NavProperty::<D>::new_reference(self.id().clone())
    }
}

impl<T: EntityTypeRef> NavProperty<T> {
    /// Extract the identifier from a navigation property.
    #[must_use]
    pub fn id(&self) -> &ODataId {
        match self {
            Self::Reference(v) => &v.odata_id,
            Self::Expanded(v) => v.0.odata_id(),
        }
    }
}

impl<T: EntityTypeRef + for<'de> Deserialize<'de> + 'static> NavProperty<T> {
    /// Get the property value.
    ///
    /// # Errors
    ///
    /// If the navigation property is already expanded then no error is returned.
    ///
    /// If the navigation is a reference then a BMC error may be returned if
    /// retrieval of the entity fails.
    pub async fn get<B: Bmc>(&self, bmc: &B) -> Result<Arc<T>, B::Error> {
        match self {
            Self::Expanded(v) => Ok(v.0.clone()),
            Self::Reference(_) => bmc.get::<T>(self.id()).await,
        }
    }

    /// Filter the property value using the provided query.
    ///
    /// # Errors
    ///
    /// Returns a BMC error if filtering the entity fails.
    #[allow(missing_docs)]
    pub async fn filter<B: Bmc>(&self, bmc: &B, query: FilterQuery) -> Result<Arc<T>, B::Error> {
        bmc.filter::<T>(self.id(), query).await
    }
}

#[cfg(test)]
mod tests {
    use super::NavProperty;
    use crate::EntityTypeRef;
    use crate::ODataETag;
    use crate::ODataId;
    use serde::Deserialize;
    use serde::Serialize;

    #[derive(Debug, Deserialize, Serialize)]
    struct DummyEntity {
        #[serde(rename = "@odata.id")]
        odata_id: ODataId,
        #[serde(rename = "Name")]
        name: String,
    }

    impl EntityTypeRef for DummyEntity {
        fn odata_id(&self) -> &ODataId {
            &self.odata_id
        }

        fn etag(&self) -> Option<&ODataETag> {
            None
        }
    }

    #[derive(Debug, Deserialize)]
    struct DefaultIdEntity {
        #[serde(rename = "@odata.id", default = "default_id")]
        odata_id: ODataId,
        #[serde(rename = "Name")]
        name: String,
    }

    impl EntityTypeRef for DefaultIdEntity {
        fn odata_id(&self) -> &ODataId {
            &self.odata_id
        }

        fn etag(&self) -> Option<&ODataETag> {
            None
        }
    }

    fn default_id() -> ODataId {
        "/default/id".to_string().into()
    }

    #[allow(dead_code)]
    #[derive(Debug, Deserialize)]
    struct StrictNameEntity {
        #[serde(rename = "@odata.id")]
        odata_id: ODataId,
        #[serde(rename = "Name")]
        name: u64,
    }

    impl EntityTypeRef for StrictNameEntity {
        fn odata_id(&self) -> &ODataId {
            &self.odata_id
        }

        fn etag(&self) -> Option<&ODataETag> {
            None
        }
    }

    #[test]
    fn nav_property_reference_for_odata_id_only_object() {
        let parsed: NavProperty<DummyEntity> =
            serde_json::from_str(r#"{ "@odata.id": "/redfish/v1/Systems/System_1" }"#).unwrap();

        let serialized = serde_json::to_value(&parsed).expect("reference must serialize");

        assert_eq!(
            serialized,
            serde_json::json!({"@odata.id": "/redfish/v1/Systems/System_1"})
        );

        match parsed {
            NavProperty::Reference(reference) => {
                assert_eq!(
                    reference.odata_id.to_string(),
                    "/redfish/v1/Systems/System_1"
                );
            }
            NavProperty::Expanded(_) => panic!("expected reference variant"),
        }
    }

    #[test]
    fn nav_property_expanded_for_object_with_extra_fields() {
        let parsed: NavProperty<DummyEntity> = serde_json::from_str(
            r#"{
                "@odata.id": "/redfish/v1/Systems/System_1",
                "Name": "System_1"
            }"#,
        )
        .unwrap();

        let serialized = serde_json::to_value(&parsed).expect("expanded property must serialize");

        assert_eq!(
            serialized,
            serde_json::json!({
                "@odata.id": "/redfish/v1/Systems/System_1",
                "Name": "System_1"
            })
        );

        match parsed {
            NavProperty::Expanded(expanded) => {
                assert_eq!(
                    expanded.0.odata_id.to_string(),
                    "/redfish/v1/Systems/System_1"
                );
                assert_eq!(expanded.0.name, "System_1");
            }
            NavProperty::Reference(_) => panic!("expected expanded variant"),
        }
    }

    #[test]
    fn nav_property_object_without_odata_id_uses_expanded_path() {
        let parsed: NavProperty<DefaultIdEntity> =
            serde_json::from_str(r#"{ "Name": "NoIdObject" }"#).unwrap();

        match parsed {
            NavProperty::Expanded(expanded) => {
                assert_eq!(expanded.0.odata_id.to_string(), "/default/id");
                assert_eq!(expanded.0.name, "NoIdObject");
            }
            NavProperty::Reference(_) => panic!("expected expanded variant"),
        }
    }

    #[test]
    fn nav_property_parse_error_for_non_reference_comes_from_t() {
        let err = serde_json::from_str::<NavProperty<StrictNameEntity>>(
            r#"{
                "@odata.id": "/redfish/v1/Systems/System_1",
                "Name": "not-a-number"
            }"#,
        )
        .unwrap_err()
        .to_string();

        assert!(
            err.contains("invalid type: string") && err.contains("u64"),
            "unexpected error: {}",
            err
        );
    }
}
