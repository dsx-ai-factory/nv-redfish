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

use crate::edmx::Annotation;
use crate::edmx::ComplexType;
use crate::edmx::NavigationProperty;
use crate::edmx::Parameter;
use crate::edmx::QualifiedTypeName;
use crate::edmx::StructuralProperty;
use crate::redfish::Deprecation;
use crate::redfish::DynamicProperties;
use crate::redfish::Excerpt;
use crate::redfish::ExcerptCopy;
use crate::redfish::ExcerptKey;
use crate::IsExcerptCopyOnly;
use crate::IsRequired;
use crate::IsRequiredOnCreate;
use std::convert::identity;

pub trait RedfishAnnotation {
    fn is_redfish_annotation(&self, name: &str) -> bool;
}

impl RedfishAnnotation for Annotation {
    fn is_redfish_annotation(&self, name: &str) -> bool {
        is_redfish_qualified_name(&self.term, name)
    }
}

fn is_redfish_qualified_name(qname: &QualifiedTypeName, name: &str) -> bool {
    qname.inner().namespace.ids.len() == 1
        && qname.inner().namespace.ids[0].inner() == "Redfish"
        && qname.inner().name.inner() == name
}

fn revisions_deprecation(annotations: &[Annotation]) -> Option<Deprecation<'_>> {
    let records = &annotations
        .iter()
        .find(|annotation| annotation.is_redfish_annotation("Revisions"))
        .and_then(|annotation| annotation.collection.as_ref())?
        .record;

    records.iter().find_map(|record| {
        record
            .property_value("Kind")?
            .enum_members()
            .any(|member| {
                is_redfish_qualified_name(&member.tname, "RevisionKind")
                    && member.mname.inner().inner() == "Deprecated"
            })
            .then(|| {
                let value = |name| {
                    record
                        .property_value(name)
                        .and_then(|value| value.string_value.as_deref())
                };
                Deprecation {
                    version: value("Version"),
                    description: value("Description"),
                }
            })
    })
}

fn legacy_deprecation(annotations: &[Annotation]) -> Option<Deprecation<'_>> {
    annotations
        .iter()
        .find(|annotation| annotation.is_redfish_annotation("Deprecated"))
        .map(|annotation| Deprecation {
            version: None,
            description: annotation.string.as_deref(),
        })
}

pub trait RedfishAnnotations {
    fn annotations(&self) -> &Vec<Annotation>;

    fn is_required(&self) -> IsRequired {
        self.annotations()
            .iter()
            .find(|a| a.is_redfish_annotation("Required"))
            .map_or_else(|| IsRequired::new(false), |_| IsRequired::new(true))
    }

    fn is_required_on_create(&self) -> IsRequiredOnCreate {
        self.annotations()
            .iter()
            .find(|a| a.is_redfish_annotation("RequiredOnCreate"))
            .map_or_else(
                || IsRequiredOnCreate::new(false),
                |_| IsRequiredOnCreate::new(true),
            )
    }

    fn is_excerpt_only(&self) -> IsExcerptCopyOnly {
        self.annotations()
            .iter()
            .find(|a| a.is_redfish_annotation("ExcerptCopyOnly"))
            .map_or_else(
                || IsExcerptCopyOnly::new(false),
                |v| IsExcerptCopyOnly::new(v.bool_value.is_none_or(identity)),
            )
    }

    /// Excerpt membership, with an explicit key list taking precedence over
    /// the implicit membership supplied by `ExcerptCopyOnly`.
    fn excerpt(&self) -> Option<Excerpt> {
        self.annotations()
            .iter()
            .find(|a| a.is_redfish_annotation("Excerpt"))
            .map(|v| {
                v.string.as_ref().map_or_else(
                    || Excerpt::All,
                    |s| Excerpt::Keys(s.split(',').map(Into::into).map(ExcerptKey::new).collect()),
                )
            })
            .or_else(|| self.is_excerpt_only().into_inner().then_some(Excerpt::All))
    }

    /// Returns if property is marked as excerpt copy.
    fn excerpt_copy(&self) -> Option<ExcerptCopy> {
        self.annotations()
            .iter()
            .find(|a| a.is_redfish_annotation("ExcerptCopy"))
            .and_then(|v| {
                v.string.as_ref().map_or_else(
                    || Some(ExcerptCopy::AllKeys),
                    |s| Some(ExcerptCopy::Key(ExcerptKey::new(s.into()))),
                )
            })
    }

    /// Return Redfish deprecation metadata.
    ///
    /// `Redfish.Revisions` is the current representation. The legacy
    /// `Redfish.Deprecated` term remains supported for older schemas.
    fn deprecation(&self) -> Option<Deprecation<'_>> {
        revisions_deprecation(self.annotations()).or_else(|| legacy_deprecation(self.annotations()))
    }

    /// Returns if type can contain dynamic properties.
    fn dynamic_properties(&self) -> Option<DynamicProperties<'_>> {
        self.annotations()
            .iter()
            .find(|a| a.is_redfish_annotation("DynamicPropertyPatterns"))
            .and_then(|v| v.collection.as_ref())
            .and_then(|collection| {
                collection.record.iter().find_map(|record| {
                    record
                        .property_value("Type")
                        .and_then(|t| t.string_value.as_ref())
                        .and_then(|t| {
                            record
                                .property_value("Pattern")
                                .and_then(|p| p.string_value.as_ref())
                                .map(|p| DynamicProperties {
                                    pattern: p,
                                    ptype: t,
                                })
                        })
                })
            })
    }
}

impl RedfishAnnotations for StructuralProperty {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl RedfishAnnotations for NavigationProperty {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl RedfishAnnotations for Parameter {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

impl RedfishAnnotations for ComplexType {
    fn annotations(&self) -> &Vec<Annotation> {
        &self.annotations
    }
}

#[cfg(test)]
mod tests {
    use super::RedfishAnnotations as _;
    use crate::edmx::StructuralProperty;
    use quick_xml::de::from_str;

    #[test]
    fn reads_current_deprecation_metadata() {
        let property: StructuralProperty = from_str(
            r#"
            <Property Name="EventType" Type="Edm.String" Nullable="false">
              <Annotation Term="Redfish.Required"/>
              <Annotation Term="Redfish.Revisions">
                <Collection>
                  <Record>
                    <PropertyValue Property="Kind" EnumMember="Redfish.RevisionKind/Added"/>
                  </Record>
                  <Record>
                    <PropertyValue Property="Kind">
                      <EnumMember>Redfish.RevisionKind/Deprecated</EnumMember>
                    </PropertyValue>
                    <PropertyValue Property="Version" String="v1_3_0"/>
                    <PropertyValue Property="Description" String="Use Other for compatibility."/>
                  </Record>
                </Collection>
              </Annotation>
            </Property>"#,
        )
        .expect("valid property");
        let deprecation = property.deprecation().expect("deprecation metadata");

        assert_eq!(deprecation.version, Some("v1_3_0"));
        assert_eq!(
            deprecation.description,
            Some("Use Other for compatibility.")
        );
        assert!(property.is_required().into_inner());
    }

    #[test]
    fn legacy_deprecation_is_supported() {
        let property: StructuralProperty = from_str(
            r#"<Property Name="Value" Type="Edm.String">
                 <Annotation Term="Redfish.Deprecated" String="Use Replacement."/>
               </Property>"#,
        )
        .expect("valid property");
        let deprecation = property.deprecation().expect("legacy deprecation");

        assert_eq!(deprecation.version, None);
        assert_eq!(deprecation.description, Some("Use Replacement."));
    }
}
