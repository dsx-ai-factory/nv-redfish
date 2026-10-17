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

//! Collect schema properties and protocol annotations without generating Rust.

use std::iter::once;

use tagged_types::TaggedType;

use super::annotation::Annotation;
use crate::compiler::redfish::RedfishProperty;
use crate::compiler::{
    Compiled, NavProperty, Property as CompiledProperty, QualifiedName, TypeDefinition,
};
use crate::edmx::PropertyName as EdmxPropertyName;
use crate::generator::common_rust::InheritedProperties;
use crate::redfish::ExcerptCopy;
use crate::{IsNullable, IsRequired, OneOrCollection};

/// A borrowed schema declaration or a generated protocol field.
#[derive(Clone, Copy)]
pub(super) enum Property<'a> {
    Structural {
        property: &'a CompiledProperty<'a>,
        type_definition: Option<&'a TypeDefinition<'a>>,
    },
    Navigation(&'a NavProperty<'a>),
    Annotation(Annotation<'a>),
    Additional,
}

/// Keep schema identifiers and protocol annotations distinct until token emission.
#[derive(Clone, Copy)]
pub(super) enum PropertyName<'a> {
    Property(&'a EdmxPropertyName),
    Annotation(Annotation<'a>),
    AdditionalProperties,
}

pub(super) fn collect<'a>(
    compiled: &'a Compiled<'a>,
    name: QualifiedName<'a>,
    inherited: &InheritedProperties<'a>,
) -> Vec<Property<'a>> {
    compiled
        .entity_types
        .get(&name)
        .into_iter()
        .flat_map(|_| Annotation::entity_metadata())
        .map(Property::Annotation)
        .chain(
            schema_properties(compiled, inherited)
                .filter(|property| {
                    !property.is_write_only()
                        && property
                            .redfish()
                            .is_none_or(|redfish| !redfish.is_excerpt_only.into_inner())
                })
                .flat_map(|property| once(property).chain(property.collection_count())),
        )
        .chain(
            inherited
                .additional_properties
                .is_some_and(TaggedType::into_inner)
                .then_some(Property::Additional),
        )
        .collect()
}

/// Only explicitly marked excerpt fields, without resource metadata or extensions.
pub(super) fn excerpt<'a>(
    compiled: &'a Compiled<'a>,
    inherited: &InheritedProperties<'a>,
    copy: &ExcerptCopy,
) -> Vec<Property<'a>> {
    schema_properties(compiled, inherited)
        .filter(|property| {
            !property.is_write_only()
                && property
                    .redfish()
                    .and_then(|redfish| redfish.excerpt.as_ref())
                    .is_some_and(|excerpt| excerpt.matches(copy))
        })
        .collect()
}

fn schema_properties<'a>(
    compiled: &'a Compiled<'a>,
    inherited: &InheritedProperties<'a>,
) -> impl Iterator<Item = Property<'a>> {
    inherited
        .structural_properties()
        .into_iter()
        .map(move |property| {
            let (_, target) = property.ptype.inner();
            Property::Structural {
                property,
                type_definition: compiled.type_definitions.get(target),
            }
        })
        .chain(
            inherited
                .navigation_properties()
                .into_values()
                .map(Property::Navigation),
        )
}

impl<'a> Property<'a> {
    const fn redfish(self) -> Option<&'a RedfishProperty<'a>> {
        match self {
            Self::Structural { property, .. } => Some(&property.redfish),
            Self::Navigation(NavProperty::Expandable(property)) => Some(&property.redfish),
            Self::Navigation(NavProperty::Reference { redfish, .. }) => Some(redfish),
            Self::Annotation(_) | Self::Additional => None,
        }
    }

    fn collection_count(self) -> Option<Self> {
        match self {
            Self::Navigation(property)
                if self.is_collection()
                    && self
                        .redfish()
                        .is_none_or(|redfish| redfish.excerpt_copy.is_none()) =>
            {
                Some(Self::Annotation(Annotation::Count(property.name())))
            }
            _ => None,
        }
    }

    fn is_write_only(self) -> bool {
        match self {
            Self::Structural { property, .. } => property.odata.permissions_is_write_only(),
            Self::Navigation(NavProperty::Expandable(property)) => {
                property.odata.permissions_is_write_only()
            }
            Self::Navigation(NavProperty::Reference { odata, .. }) => {
                odata.permissions_is_write_only()
            }
            Self::Annotation(_) | Self::Additional => false,
        }
    }

    pub(super) const fn name(self) -> PropertyName<'a> {
        match self {
            Self::Structural { property, .. } => PropertyName::Property(property.name),
            Self::Navigation(property) => PropertyName::Property(property.name()),
            Self::Annotation(annotation) => PropertyName::Annotation(annotation),
            Self::Additional => PropertyName::AdditionalProperties,
        }
    }

    pub(super) const fn required(self) -> IsRequired {
        match self {
            Self::Structural { property, .. } => property.redfish.is_required,
            Self::Navigation(NavProperty::Expandable(property)) => property.redfish.is_required,
            Self::Navigation(NavProperty::Reference { redfish, .. }) => redfish.is_required,
            Self::Annotation(annotation) => IsRequired::new(matches!(
                annotation,
                Annotation::ODataId | Annotation::ODataType,
            )),
            Self::Additional => IsRequired::new(false),
        }
    }

    pub(super) const fn nullable(self) -> IsNullable {
        match self {
            Self::Structural { property, .. } => property.nullable,
            Self::Navigation(NavProperty::Expandable(property)) => property.nullable,
            Self::Navigation(NavProperty::Reference { nullable, .. }) => *nullable,
            Self::Annotation(_) | Self::Additional => IsNullable::new(false),
        }
    }

    pub(super) const fn is_collection(self) -> bool {
        match self {
            Self::Structural { property, .. } => {
                matches!(property.ptype, OneOrCollection::Collection(_))
            }
            Self::Navigation(NavProperty::Expandable(property)) => {
                matches!(property.ptype, OneOrCollection::Collection(_))
            }
            Self::Navigation(NavProperty::Reference { cardinality, .. }) => {
                matches!(cardinality, OneOrCollection::Collection(_))
            }
            Self::Annotation(_) | Self::Additional => false,
        }
    }

    pub(super) fn constructor_required(self) -> bool {
        self.required().into_inner() && !matches!(self, Self::Annotation(Annotation::ODataType))
    }
}
