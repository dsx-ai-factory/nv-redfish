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

//! EDMX parser and validator.

/// 3.1 Element edmx:Edmx
pub mod edmx_root;

/// 3.2 Element edmx:DataServicse
pub mod data_services;

/// 3.3 Element edmx:Reference
pub mod reference;

/// 3.4 Element edmx:Include
pub mod include;

/// 3.5 Element edmx:IncludeAnnotations
pub mod include_annotations;

/// 5 Schema
pub mod schema;

/// 6 Structural Property / 7 Navigation Property
pub mod property;

/// 8 Entity Type
pub mod entity_type;

/// 9 Complex Type
pub mod complex_type;

/// 10 Enumeration Type
pub mod enum_type;

/// 12.1 Element edm:Action
pub mod action;

/// 14.3 Element edm:Annotation
pub mod annotation;

/// 17 Attribute Values
pub mod attribute_values;

/// Validation errors.
pub mod validate_error;

use serde::Deserialize;
use tagged_types::TaggedType;

#[doc(inline)]
pub use action::Action;
#[doc(inline)]
pub use annotation::Annotation;
#[doc(inline)]
pub use annotation::AnnotationRecord;
#[doc(inline)]
pub use attribute_values::Namespace;
#[doc(inline)]
pub use attribute_values::QualifiedName;
#[doc(inline)]
pub use attribute_values::SimpleIdentifier;
#[doc(inline)]
pub use attribute_values::TypeName;
#[doc(inline)]
pub use complex_type::ComplexType;
#[doc(inline)]
pub use edmx_root::Edmx;
#[doc(inline)]
pub use entity_type::EntityType;
#[doc(inline)]
pub use enum_type::EnumMember;
#[doc(inline)]
pub use enum_type::EnumMemberName;
#[doc(inline)]
pub use enum_type::EnumType;
#[doc(inline)]
pub use enum_type::EnumUnderlyingType;
#[doc(inline)]
pub use property::NavigationProperty;
#[doc(inline)]
pub use property::Property;
#[doc(inline)]
pub use property::StructuralProperty;
#[doc(inline)]
pub use schema::Schema;
#[doc(inline)]
pub use schema::Type;
#[doc(inline)]
pub use validate_error::ValidateError;

use crate::IsNullable;

/// Qualified type name is type name rogether with the namespace.
pub type QualifiedTypeName = TaggedType<QualifiedName, QualifiedTypeNameTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Hash, PartialEq, Eq)]
#[transparent(Debug, FromStr, Display, Deserialize)]
#[capability(inner_access)]
pub enum QualifiedTypeNameTag {}

/// This is name of type inside Schema. This type is used when types
/// are defined.
pub type LocalTypeName = TaggedType<SimpleIdentifier, LocalTypeNameTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Hash, PartialEq, Eq)]
#[transparent(Debug, Display, Deserialize)]
#[capability(inner_access)]
pub enum LocalTypeNameTag {}

/// Name of the Action.
pub type ActionName = TaggedType<SimpleIdentifier, ActionNameTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Deserialize)]
#[capability(inner_access)]
pub enum ActionNameTag {}

/// Name of the Property (either structural and navigational).
pub type PropertyName = TaggedType<SimpleIdentifier, PropertyNameTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[transparent(Debug, Display, Deserialize, FromStr)]
#[capability(inner_access)]
pub enum PropertyNameTag {}

/// Name of the Parameter of `Function` or `Action`. In Redfish we
/// don't have functions so here it is only parameter of Actions.
pub type ParameterName = TaggedType<SimpleIdentifier, ParameterNameTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[transparent(Debug, Display, Deserialize)]
#[capability(inner_access)]
pub enum ParameterNameTag {}

/// Flag for Action that says that action is bound to the specific
/// type. First paramer of such Action defines binding.
pub type IsBound = TaggedType<bool, IsBoundTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Copy, Clone)]
#[transparent(Debug, Deserialize)]
#[capability(inner_access)]
pub enum IsBoundTag {}

/// 11.1 Element edm:TypeDefinition
#[derive(Debug, Deserialize)]
pub struct TypeDefinition {
    /// 11.1.1 Attribute Name
    #[serde(rename = "@Name")]
    pub name: LocalTypeName,
    /// 11.1.2 Attribute `UnderlyingType`
    ///
    /// Note that we can narrow down this type from
    /// `QualifiedTypeName` to primitive types starting with `Edm`
    /// prefix.
    #[serde(rename = "@UnderlyingType")]
    pub underlying_type: QualifiedTypeName,
    /// Annotations can be pretty much everywhere.
    #[serde(rename = "Annotation", default)]
    pub annotations: Vec<Annotation>,
}

/// 13.2 Element edm:EntitySet
#[derive(Debug, Deserialize)]
pub struct EntityContainer {
    /// 13.2.1 Attribute Name
    #[serde(rename = "@Name")]
    pub name: LocalTypeName,
    /// 13.3 Element edm:Singleton
    ///
    /// This is the only used element of edm:EntityContainer in
    /// Redfish.
    #[serde(rename = "Singleton", default)]
    pub singletons: Vec<Singleton>,
}

/// 13.3 Element edm:Singleton
#[derive(Debug, Deserialize)]
pub struct Singleton {
    /// 13.3.1 Attribute Name
    #[serde(rename = "@Name")]
    pub name: SimpleIdentifier,
    /// 13.3.2 Attribute Type
    #[serde(rename = "@Type")]
    pub stype: QualifiedTypeName,
}

/// 14.1 Element edm:Term
#[derive(Debug, Deserialize)]
pub struct Term {
    /// 14.1.1 Attribute `Name`
    #[serde(rename = "@Name")]
    pub name: LocalTypeName,
    /// 14.1.2 Attribute `Type`
    #[serde(rename = "@Type")]
    pub ttype: Option<TypeName>,
    /// 14.1.4 Attribute `DefaultValue`
    #[serde(rename = "@DefaultValue")]
    pub default_value: Option<String>,
    /// Annotations can be pretty much everywhere.
    #[serde(rename = "Annotation", default)]
    pub annotations: Vec<Annotation>,
}

/// 12.3 Element edm:ReturnType
#[derive(Debug, Deserialize)]
pub struct ReturnType {
    /// 12.3.1 Attribute Type
    #[serde(rename = "@Type")]
    pub rtype: TypeName,
    /// 12.3.2 Attribute Nullable
    #[serde(rename = "@Nullable")]
    pub nullable: Option<IsNullable>,
}

/// 12.4 Element edm:Parameter
#[derive(Debug, Deserialize)]
pub struct Parameter {
    /// 12.4.1 Attribute Name
    #[serde(rename = "@Name")]
    pub name: ParameterName,
    /// 12.4.2 Attribute Type
    #[serde(rename = "@Type")]
    pub ptype: TypeName,
    /// 12.4.3 Attribute Nullable
    #[serde(rename = "@Nullable")]
    pub nullable: Option<IsNullable>,
    /// Annotations can be pretty much everywhere.
    #[serde(rename = "Annotation", default)]
    pub annotations: Vec<Annotation>,
}
