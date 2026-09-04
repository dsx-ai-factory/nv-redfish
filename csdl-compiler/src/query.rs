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

//! Schema queries for projection compilers.
//!
//! [`SchemaQuery::resolve`](crate::query::SchemaQuery::resolve) answers what a path *is*;
//! [`SchemaQuery::steps`](crate::query::SchemaQuery::steps) additionally answers how generated
//! Rust reaches it, segment by segment.

use std::collections::HashMap;

use crate::compiler::Compiled;
use crate::compiler::Config;
use crate::compiler::EntityTypeFilter;
use crate::compiler::Error;
use crate::compiler::Properties;
use crate::compiler::Property;
use crate::compiler::QualifiedName;
use crate::compiler::SchemaBundle;
use crate::compiler::TypeClass;
use crate::edmx::EnumMemberName;
use crate::optimizer::optimize;
use crate::optimizer::Config as OptimizerConfig;
use crate::OneOrCollection;

/// One resolved structural property.
#[derive(Debug)]
pub struct ResolvedProperty<'a> {
    pub type_name: QualifiedName<'a>,
    pub class: TypeClass,
    pub nullable: bool,
    pub collection: bool,
    pub enum_members: Option<Vec<&'a EnumMemberName>>,
}

/// One segment of a resolved property path.
///
/// The last step is the leaf, carrying the same facts
/// [`SchemaQuery::resolve`] reports; every step additionally carries what
/// generated field access needs — the declaring type's distance up the
/// base chain, `Redfish.Required`, and a type definition's underlying
/// primitive.
#[derive(Debug)]
#[non_exhaustive]
pub struct Step<'a> {
    /// The property's type after the fold.
    pub type_name: QualifiedName<'a>,
    pub class: TypeClass,
    pub nullable: bool,
    pub collection: bool,
    /// `Redfish.Required` on the property.
    pub required: bool,
    /// Base-chain hops from the holding type to the type declaring the
    /// property, in the optimizer-collapsed chain — the count of `base`
    /// accesses generated field access puts in front of the field.
    pub hops: usize,
    /// For a type definition, the primitive underneath.
    pub underlying: Option<QualifiedName<'a>>,
    /// Members when the type is an enum.
    pub enum_members: Option<Vec<&'a EnumMemberName>>,
}

impl<'a> From<Step<'a>> for ResolvedProperty<'a> {
    fn from(leaf: Step<'a>) -> Self {
        Self {
            type_name: leaf.type_name,
            class: leaf.class,
            nullable: leaf.nullable,
            collection: leaf.collection,
            enum_members: leaf.enum_members,
        }
    }
}

/// A queryable view over a compiled bundle.
pub struct SchemaQuery<'a> {
    compiled: Compiled<'a>,
    entities: HashMap<&'a str, QualifiedName<'a>>,
}

impl<'a> SchemaQuery<'a> {
    /// Compiles the whole bundle and indexes its entity types.
    ///
    /// # Errors
    ///
    /// Returns a compile error if any type cannot be resolved.
    pub fn build(bundle: &'a SchemaBundle) -> Result<Self, Error<'a>> {
        let config = Config {
            entity_type_filter: EntityTypeFilter::new_restrictive(Vec::new()),
            ..Config::default()
        };
        let compiled = bundle.compile_all(config)?;
        let compiled = optimize(compiled, &OptimizerConfig::default());

        let mut entities = HashMap::new();
        for qname in compiled.entity_types.keys() {
            let name = qname.name.inner().as_str();
            let replace = entities
                .get(name)
                .is_none_or(|current| Rank::new(*qname) > Rank::new(*current));
            if replace {
                entities.insert(name, *qname);
            }
        }
        Ok(Self { compiled, entities })
    }

    /// Whether the bundle declares an entity type of this name.
    #[must_use]
    pub fn has_entity(&self, name: &str) -> bool {
        self.entity(name).is_some()
    }

    /// The qualified name the index picked for an entity short name: the
    /// schema family rooted at the name at its highest version when one
    /// exists, otherwise the highest-ranked declaration of the name.
    #[must_use]
    pub fn entity(&self, name: &str) -> Option<QualifiedName<'a>> {
        self.entities.get(name).copied()
    }

    /// Resolves a dotted property path against the named entity type,
    /// descending through singular complex types and walking base chains.
    #[must_use]
    pub fn resolve(&self, entity: &str, path: &str) -> Option<ResolvedProperty<'a>> {
        self.steps(entity, path)?.pop().map(Into::into)
    }

    /// Resolves a dotted property path into one step per segment.
    #[must_use]
    pub fn steps(&self, entity: &str, path: &str) -> Option<Vec<Step<'a>>> {
        let mut current = TypeRef::Entity(*self.entities.get(entity)?);
        let mut steps = Vec::new();
        let mut segments = path.split('.').peekable();
        while let Some(segment) = segments.next() {
            let (property, hops) = self.property_of(current, segment)?;
            let ((info, qname), collection) = match &property.ptype {
                OneOrCollection::One(inner) => (inner, false),
                OneOrCollection::Collection(inner) => (inner, true),
            };
            let (class, qname) = (info.class, *qname);
            let enum_members =
                match class {
                    TypeClass::EnumType => self.compiled.enum_types.get(&qname).map(|declared| {
                        declared.members.iter().map(|member| member.name).collect()
                    }),
                    _ => None,
                };
            let underlying = match class {
                TypeClass::TypeDefinition => self
                    .compiled
                    .type_definitions
                    .get(&qname)
                    .map(|definition| definition.underlying_type),
                _ => None,
            };
            steps.push(Step {
                type_name: qname,
                class,
                nullable: property.nullable.into_inner(),
                collection,
                required: property.redfish.is_required.into_inner(),
                hops,
                underlying,
                enum_members,
            });
            if segments.peek().is_none() {
                return Some(steps);
            }
            if class != TypeClass::ComplexType || collection {
                return None;
            }
            current = TypeRef::Complex(qname);
        }
        None
    }

    /// The named structural property with the number of base-chain hops
    /// that reached it.
    fn property_of(&self, mut tref: TypeRef<'a>, name: &str) -> Option<(&Property<'a>, usize)> {
        let mut hops = 0;
        while let Some((properties, base)) = self.declaration(tref) {
            if let Some(property) = properties
                .properties
                .iter()
                .find(|property| property.name.inner().inner() == name)
            {
                return Some((property, hops));
            }
            hops += 1;
            tref = match tref {
                TypeRef::Entity(_) => TypeRef::Entity(base?),
                TypeRef::Complex(_) => TypeRef::Complex(base?),
            };
        }
        None
    }

    fn declaration(
        &self,
        tref: TypeRef<'a>,
    ) -> Option<(&Properties<'a>, Option<QualifiedName<'a>>)> {
        match tref {
            TypeRef::Entity(qname) => self
                .compiled
                .entity_types
                .get(&qname)
                .map(|entity| (&entity.properties, entity.base)),
            TypeRef::Complex(qname) => self
                .compiled
                .complex_types
                .get(&qname)
                .map(|complex| (&complex.properties, complex.base)),
        }
    }
}

#[derive(Clone, Copy)]
enum TypeRef<'a> {
    Entity(QualifiedName<'a>),
    Complex(QualifiedName<'a>),
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct Rank<'a> {
    own_family: bool,
    version: Option<Version>,
    qname: QualifiedName<'a>,
}

impl<'a> Rank<'a> {
    fn new(qname: QualifiedName<'a>) -> Self {
        Self {
            own_family: qname.namespace.get_id(0) == Some(qname.name),
            version: qname
                .namespace
                .get_id(1)
                .and_then(|id| Version::parse(id.inner().as_str())),
            qname,
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
}

impl Version {
    fn parse(id: &str) -> Option<Self> {
        let mut parts = id.strip_prefix('v')?.split('_').map(Self::number);
        let version = Self {
            major: parts.next()??,
            minor: parts.next()??,
            patch: parts.next()??,
        };
        parts.next().is_none().then_some(version)
    }

    fn number(part: &str) -> Option<u64> {
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::edmx::Edmx;

    fn bundle() -> SchemaBundle {
        // `compile_all` unconditionally compiles the Redfish framework
        // types, so the fixture carries minimal Resource and Settings
        // declarations beside the types under test.
        let schema = r#"<edmx:Edmx Version="4.0">
          <edmx:DataServices>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Resource">
              <EntityType Name="Resource" Abstract="true">
                <Property Name="Id" Type="Edm.String" Nullable="false"/>
              </EntityType>
              <EntityType Name="ResourceCollection" Abstract="true"/>
              <ComplexType Name="Status">
                <Property Name="Health" Type="Resource.Health" Nullable="true"/>
              </ComplexType>
              <EnumType Name="Health">
                <Member Name="OK"/>
                <Member Name="Warning"/>
              </EnumType>
              <TypeDefinition Name="Label" UnderlyingType="Edm.String"/>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Settings">
              <ComplexType Name="Settings"/>
              <ComplexType Name="PreferredApplyTime"/>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Widget">
              <EntityType Name="Widget" Abstract="true" BaseType="Resource.Resource"/>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Widget.v1_0_0">
              <EntityType Name="Widget" BaseType="Widget.Widget">
                <Property Name="Reading" Type="Edm.Decimal" Nullable="true"/>
                <Property Name="Status" Type="Resource.Status" Nullable="false"/>
                <Property Name="Tag" Type="Resource.Label" Nullable="false">
                  <Annotation Term="Redfish.Required"/>
                </Property>
              </EntityType>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Widget.v1_1_0">
              <EntityType Name="Widget" BaseType="Widget.v1_0_0.Widget">
                <Property Name="Labels" Type="Collection(Edm.String)" Nullable="false"/>
                <Property Name="Slots" Type="Collection(Resource.Status)" Nullable="false"/>
              </EntityType>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Widget.v1_10_0">
              <EntityType Name="Widget" BaseType="Widget.v1_1_0.Widget">
                <Property Name="Deca" Type="Edm.Boolean" Nullable="false"/>
              </EntityType>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Cabinet">
              <EntityType Name="Widget" Abstract="true" BaseType="Resource.Resource"/>
            </Schema>
            <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Cabinet.v9_9_9">
              <EntityType Name="Widget" BaseType="Cabinet.Widget">
                <Property Name="Legacy" Type="Edm.String" Nullable="false"/>
              </EntityType>
            </Schema>
          </edmx:DataServices>
        </edmx:Edmx>"#;

        SchemaBundle::new(
            vec![Edmx::parse(schema).expect("query test schema must be valid")],
            Vec::new(),
        )
    }

    #[test]
    fn versions_rank_numerically_and_reject_loose_grammar() {
        assert!(Version::parse("v1_10_0") > Version::parse("v1_9_9"));
        assert!(Version::parse("v1_0_0") > None);
        for loose in ["v+1_2_3", "v1_2", "v1_2_3_4", "v1_2_x", "1_2_3", "v1__3"] {
            assert!(Version::parse(loose).is_none(), "{} must not parse", loose);
        }
    }

    #[test]
    fn resolves_across_versions_bases_and_complex_types() {
        let bundle = bundle();
        let query = SchemaQuery::build(&bundle).expect("bundle compiles");

        assert!(query.has_entity("Widget"));
        assert!(!query.has_entity("Gadget"));

        // Declared in v1_0_0, visible from the most derived fold.
        let reading = query
            .resolve("Widget", "Reading")
            .expect("Reading resolves");
        assert_eq!(reading.type_name.to_string(), "Edm.Decimal");
        assert_eq!(reading.class, TypeClass::SimpleType);
        assert!(reading.nullable);
        assert!(!reading.collection);

        // Declared on the abstract base of the base.
        let id = query.resolve("Widget", "Id").expect("Id resolves");
        assert!(!id.nullable);

        // Added in v1_1_0, visible through v1_10_0's base chain.
        let labels = query.resolve("Widget", "Labels").expect("Labels resolve");
        assert!(labels.collection);

        // The index ranks versions numerically: v1_10_0 outranks v1_1_0,
        // which plain string order would invert.
        assert!(query.resolve("Widget", "Deca").is_some());

        // `Cabinet.v9_9_9.Widget` shares the short name at a higher
        // version, but `Widget` means the family rooted at `Widget`.
        assert!(query.resolve("Widget", "Legacy").is_none());

        // Through a complex type to an enum, members included.
        let health = query
            .resolve("Widget", "Status.Health")
            .expect("Status.Health resolves");
        assert_eq!(health.class, TypeClass::EnumType);
        let members: Vec<String> = health
            .enum_members
            .expect("an enum has members")
            .iter()
            .map(|member| member.inner().to_string())
            .collect();
        assert_eq!(members, ["OK", "Warning"]);

        // Unknown paths and descent through scalars resolve to nothing.
        assert!(query.resolve("Widget", "Readng").is_none());
        assert!(query.resolve("Widget", "Reading.Deeper").is_none());
    }

    #[test]
    fn steps_expose_per_segment_emission_facts() {
        let bundle = bundle();
        let query = SchemaQuery::build(&bundle).expect("bundle compiles");

        // The pick behind every walk is addressable by short name; its
        // namespace after the fold spells the generated module.
        let widget = query.entity("Widget").expect("Widget is indexed");
        assert_eq!(widget.namespace.to_string(), "Widget");
        assert!(query.entity("Gadget").is_none());

        // A leaf on the fold itself: no hops. One declared on a base:
        // as many hops as generated field access spells `base`.
        let reading = query.steps("Widget", "Reading").expect("Reading resolves");
        assert_eq!(reading.len(), 1);
        assert_eq!(reading[0].hops, 0);
        assert!(!reading[0].required);
        let id = query.steps("Widget", "Id").expect("Id resolves");
        assert_eq!(id[0].hops, 1);

        // A required type definition: the annotation and the underlying
        // primitive, neither of which the leaf view reports.
        let tag = query.steps("Widget", "Tag").expect("Tag resolves");
        assert!(tag[0].required);
        assert!(!tag[0].nullable);
        assert_eq!(tag[0].class, TypeClass::TypeDefinition);
        let underlying = tag[0].underlying.expect("a typedef has an underlying type");
        assert_eq!(underlying.to_string(), "Edm.String");

        // Every segment reports, not only the leaf; the leaf agrees with
        // `resolve` because it is `resolve`.
        let steps = query
            .steps("Widget", "Status.Health")
            .expect("Status.Health resolves");
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].class, TypeClass::ComplexType);
        assert_eq!(steps[0].hops, 0);
        assert!(!steps[0].nullable);
        assert!(steps[1].nullable);
        let leaf = query
            .resolve("Widget", "Status.Health")
            .expect("Status.Health resolves");
        assert_eq!(leaf.type_name, steps[1].type_name);
        assert_eq!(
            leaf.enum_members.expect("Health is an enum").len(),
            steps[1].enum_members.as_ref().expect("same walk").len()
        );

        // A collection leaf is addressable; descent through one is not —
        // its element is no property holder in generated Rust.
        assert!(query.steps("Widget", "Slots").is_some());
        assert!(query.steps("Widget", "Slots.Health").is_none());
        assert!(query.resolve("Widget", "Slots.Health").is_none());
    }
}
