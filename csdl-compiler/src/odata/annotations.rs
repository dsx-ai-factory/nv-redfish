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

//! Helper of handling annotations in edmx types.

use crate::edmx::Action;
use crate::edmx::Annotation;
use crate::edmx::AnnotationRecord;
use crate::edmx::ComplexType;
use crate::edmx::EntityType;
use crate::edmx::EnumMember;
use crate::edmx::EnumType;
use crate::edmx::Namespace;
use crate::edmx::NavigationProperty;
use crate::edmx::Parameter;
use crate::edmx::StructuralProperty;
use crate::edmx::Term;
use tagged_types::TaggedType;

/// A brief description of a model element.
pub type DescriptionRef<'a> = TaggedType<&'a String, DescriptionTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug)]
#[capability(inner_access, cloned)]
pub enum DescriptionTag {}

/// A lengthy description of a model element.
pub type LongDescriptionRef<'a> = TaggedType<&'a String, LongDescriptionTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug)]
#[capability(inner_access, cloned)]
pub enum LongDescriptionTag {}

/// Instances of this type may contain properties in addition to those
/// declared in `$metadata`.
pub type AdditionalProperties = TaggedType<bool, AdditionalPropertiesTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug)]
#[capability(inner_access, cloned)]
pub enum AdditionalPropertiesTag {}

/// Capabilities of Enity type
#[derive(Debug, Clone, Copy)]
pub struct Capability<'a> {
    pub value: bool,
    pub description: Option<DescriptionRef<'a>>,
}

/// Enitity type is insertable.
pub type Insertable<'a> = TaggedType<Capability<'a>, InsertableTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum InsertableTag {}

/// Enitity type is updatable.
pub type Updatable<'a> = TaggedType<Capability<'a>, UpdatableTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum UpdatableTag {}

/// Enitity type is deletable.
pub type Deletable<'a> = TaggedType<Capability<'a>, DeletableTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum DeletableTag {}

/// Permissions for accessing a resource.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permissions {
    #[default]
    Read,
    Write,
    ReadWrite,
}

trait IsODataNamespace {
    fn is_odata_namespace(&self) -> bool;
    fn is_capabilities_namespace(&self) -> bool;
}

impl IsODataNamespace for Namespace {
    fn is_odata_namespace(&self) -> bool {
        self.ids.len() == 1 && self.ids[0].inner() == "OData"
    }
    fn is_capabilities_namespace(&self) -> bool {
        self.ids.len() == 1 && self.ids[0].inner() == "Capabilities"
    }
}

pub trait ODataAnnotation {
    fn is_odata_annotation(&self, name: &str) -> bool;
    fn is_capabilities_annotation(&self, name: &str) -> bool;
}

impl ODataAnnotation for Annotation {
    fn is_odata_annotation(&self, name: &str) -> bool {
        self.term.inner().namespace.is_odata_namespace() && self.term.inner().name.inner() == name
    }
    fn is_capabilities_annotation(&self, name: &str) -> bool {
        self.term.inner().namespace.is_capabilities_namespace()
            && self.term.inner().name.inner() == name
    }
}

pub trait ODataAnnotations {
    fn annotations(&self) -> &Vec<Annotation>;

    fn odata_description(&self) -> Option<DescriptionRef<'_>> {
        self.annotations()
            .iter()
            .find(|a| a.is_odata_annotation("Description"))
            .and_then(|a| a.string.as_ref())
            .map(DescriptionRef::new)
    }

    fn odata_long_description(&self) -> Option<LongDescriptionRef<'_>> {
        self.annotations()
            .iter()
            .find(|a| a.is_odata_annotation("LongDescription"))
            .and_then(|a| a.string.as_ref())
            .map(LongDescriptionRef::new)
    }

    fn odata_additional_properties(&self) -> Option<AdditionalProperties> {
        self.annotations()
            .iter()
            .find(|a| a.is_odata_annotation("AdditionalProperties"))
            .and_then(|a| a.bool_value)
            .map(AdditionalProperties::new)
    }

    fn odata_permissions(&self) -> Option<Permissions> {
        self.annotations()
            .iter()
            .find(|a| a.is_odata_annotation("Permissions"))
            .and_then(|a| a.enum_members().next())
            .and_then(|v| match v.mname.inner().inner().as_str() {
                "ReadWrite" => Some(Permissions::ReadWrite),
                "Read" => Some(Permissions::Read),
                "Write" => Some(Permissions::Write),
                _ => None,
            })
    }

    fn odata_is_url(&self) -> bool {
        self.annotations()
            .iter()
            .any(|annotation| annotation.is_odata_annotation("IsURL"))
    }

    fn capabilities_insertable(&self) -> Option<Insertable<'_>> {
        self.annotations()
            .iter()
            .find(|a| a.is_capabilities_annotation("InsertRestrictions"))
            .and_then(|a| a.record.as_ref())
            .and_then(|record| {
                record.property_value("Insertable").and_then(|pv| {
                    pv.bool_value.map(|value| Capability {
                        value,
                        description: record.odata_description(),
                    })
                })
            })
            .map(Insertable::new)
    }

    fn capabilities_updatable(&self) -> Option<Updatable<'_>> {
        self.annotations()
            .iter()
            .find(|a| a.is_capabilities_annotation("UpdateRestrictions"))
            .and_then(|a| a.record.as_ref())
            .and_then(|record| {
                record.property_value("Updatable").and_then(|pv| {
                    pv.bool_value.map(|value| Capability {
                        value,
                        description: record.odata_description(),
                    })
                })
            })
            .map(Updatable::new)
    }

    fn capabilities_deletable(&self) -> Option<Deletable<'_>> {
        self.annotations()
            .iter()
            .find(|a| a.is_capabilities_annotation("DeleteRestrictions"))
            .and_then(|a| a.record.as_ref())
            .and_then(|record| {
                record.property_value("Deletable").and_then(|pv| {
                    pv.bool_value.map(|value| Capability {
                        value,
                        description: record.odata_description(),
                    })
                })
            })
            .map(Deletable::new)
    }
}

impl ODataAnnotations for EnumType {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for Term {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for EnumMember {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for EntityType {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for ComplexType {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for StructuralProperty {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for NavigationProperty {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for Action {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for Parameter {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl ODataAnnotations for AnnotationRecord {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}
