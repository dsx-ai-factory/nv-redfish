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

use crate::compiler::Error;
use crate::compiler::Namespace;
use crate::compiler::QualifiedName;
use crate::edmx::ComplexType;
use crate::edmx::Edmx;
use crate::edmx::EntityType;
use crate::edmx::Namespace as EdmxNamespace;
use crate::edmx::Schema;
use crate::edmx::SimpleIdentifier;
use crate::edmx::Type;
use std::collections::HashMap;
use std::collections::HashSet;
use std::convert::identity;

/// Index over schemas spanning multiple documents.
pub struct SchemaIndex<'a> {
    index: HashMap<Namespace<'a>, &'a Schema>,
    /// Mapping from base types to all inherited types. This index is
    /// built for complex and entity types.
    child_map: HashMap<QualifiedName<'a>, Vec<QualifiedName<'a>>>,
}

impl<'a> SchemaIndex<'a> {
    /// Build an index from the provided documents.
    ///
    /// # Errors
    ///
    /// Returns an error if entity or complex type inheritance contains a cycle.
    pub fn build(edmx_docs: impl IntoIterator<Item = &'a Edmx> + Clone) -> Result<Self, Error<'a>> {
        let index = edmx_docs
            .clone()
            .into_iter()
            .flat_map(|v| {
                v.data_services
                    .schemas
                    .iter()
                    .map(|s| (Namespace::new(&s.namespace), s))
            })
            .collect();

        let (child_map, base_map) = edmx_docs.into_iter().fold(
            (
                HashMap::<QualifiedName<'a>, Vec<QualifiedName<'a>>>::new(),
                HashMap::<QualifiedName<'a>, QualifiedName<'a>>::new(),
            ),
            |maps, doc| {
                doc.data_services.schemas.iter().fold(maps, |map, s| {
                    let entity_types = s
                        .entity_types
                        .values()
                        .filter_map(|et| et.base_type.as_ref().map(|base| (&et.name, base)));
                    let complex_types = s.types.values().filter_map(|t| {
                        if let Type::ComplexType(ct) = &t {
                            ct.base_type.as_ref().map(|base| (&ct.name, base))
                        } else {
                            None
                        }
                    });
                    entity_types.chain(complex_types).fold(
                        map,
                        |(mut child_map, mut base_map), (name, base)| {
                            let qname = QualifiedName::new(&s.namespace, name.inner());
                            let base_type: QualifiedName = base.into();
                            child_map
                                .entry(base_type)
                                .and_modify(|e| e.push(qname))
                                .or_insert_with(|| vec![qname]);
                            base_map.insert(qname, base_type);
                            (child_map, base_map)
                        },
                    )
                })
            },
        );
        find_inheritance_cycle(&base_map).map_or(Ok(Self { index, child_map }), |cycle| {
            Err(Error::CyclicType(cycle))
        })
    }

    /// Find schema by namespace.
    #[must_use]
    pub fn get(&self, ns: &Namespace<'_>) -> Option<&'a Schema> {
        self.index.get(ns).map(|v| &**v)
    }

    /// Find an entity type by its qualified name.
    #[must_use]
    pub fn find_entity_type(&self, qtype: QualifiedName<'_>) -> Option<&'a EntityType> {
        self.get(&qtype.namespace)
            .and_then(|ns| ns.entity_types.get(qtype.name))
    }

    /// Find the most specific child entity type.
    ///
    /// # Errors
    ///
    /// Returns an error if the entity type is not found.
    pub fn find_child_entity_type(
        &self,
        qtype: QualifiedName<'a>,
    ) -> Result<(QualifiedName<'a>, &'a EntityType), Error<'a>> {
        let qtype = self.find_child_type(qtype);
        self.get(&qtype.namespace)
            .and_then(|ns| ns.entity_types.get(qtype.name))
            .ok_or(Error::EntityTypeNotFound(qtype))
            .map(|v| (qtype, v))
    }

    /// Find the most specific child complex type.
    ///
    /// # Errors
    ///
    /// Returns an error if the complex type is not found.
    pub fn find_child_complex_type(
        &self,
        qtype: QualifiedName<'a>,
    ) -> Result<(QualifiedName<'a>, &'a ComplexType), Error<'a>> {
        let qtype = self.find_child_type(qtype);
        self.get(&qtype.namespace)
            .and_then(|ns| ns.types.get(qtype.name))
            .and_then(|t| {
                if let Type::ComplexType(ct) = t {
                    Some(ct)
                } else {
                    None
                }
            })
            .ok_or(Error::ComplexTypeNotFound(qtype))
            .map(|v| (qtype, v))
    }

    /// Find a type by its qualified name.
    #[must_use]
    pub fn find_type(&self, qtype: QualifiedName<'_>) -> Option<&'a Type> {
        self.get(&qtype.namespace)
            .and_then(|ns| ns.types.get(qtype.name))
    }

    /// Find a child type by qualified name. For complex/entity types,
    /// returns the most distant unique descendant; otherwise returns
    /// the input type unchanged.
    #[must_use]
    pub fn find_child_type(&self, mut qtype: QualifiedName<'a>) -> QualifiedName<'a> {
        while let Some(children) = self.child_map.get(&qtype) {
            let children = children
                .iter()
                .filter(|child| self.child_adds_property(child))
                .copied()
                .collect::<Vec<_>>();
            if children.len() > 1 {
                break;
            }
            if let Some(child) = children.first() {
                qtype = *child;
            } else {
                break;
            }
        }
        qtype
    }

    /// Find the `Settings.Settings` type corresponding to the
    /// `@Redfish.Settings` annotation.
    ///
    /// # Errors
    ///
    /// Returns an error if the settings type is not found.
    ///
    /// # Panics
    ///
    /// Should never panic unless the EDMX `SimpleIdentifier` parser is broken.
    #[allow(clippy::unwrap_in_result)]
    pub fn redfish_settings_type(&self) -> Result<(QualifiedName<'a>, &'a ComplexType), Error<'a>> {
        let ns: EdmxNamespace = "Settings".parse().expect("must be parsed");
        let id: SimpleIdentifier = "Settings".parse().expect("must be parsed");
        let schema = self
            .get(&Namespace::new(&ns))
            .ok_or(Error::SettingsTypeNotFound)?;
        let (name, _) = schema
            .types
            .get_key_value(&id)
            .ok_or(Error::SettingsTypeNotFound)?;
        let qtype = QualifiedName::new(&schema.namespace, name);
        self.find_child_complex_type(qtype)
    }

    /// Find the `Settings.PreferredApplyTime` type corresponding to
    /// the `@Redfish.SettingsApplyTime` annotation.
    ///
    /// # Errors
    ///
    /// Returns an error if the settings type is not found.
    ///
    /// # Panics
    ///
    /// Should never panic unless the EDMX `SimpleIdentifier` parser is broken.
    #[allow(clippy::unwrap_in_result)]
    pub fn redfish_settings_preferred_apply_time_type(
        &self,
    ) -> Result<(QualifiedName<'a>, &'a ComplexType), Error<'a>> {
        let ns: EdmxNamespace = "Settings".parse().expect("must be parsed");
        let id: SimpleIdentifier = "PreferredApplyTime".parse().expect("must be parsed");
        let schema = self
            .get(&Namespace::new(&ns))
            .ok_or(Error::SettingsPreferredApplyTimeTypeNotFound)?;
        let (name, _) = schema
            .types
            .get_key_value(&id)
            .ok_or(Error::SettingsPreferredApplyTimeTypeNotFound)?;
        let qtype = QualifiedName::new(&schema.namespace, name);
        self.find_child_complex_type(qtype)
    }

    /// Find the `Resource.Resource` type corresponding that is base
    /// type for all Redfish resources
    ///
    /// # Errors
    ///
    /// Returns an error if the type is not found.
    ///
    /// # Panics
    ///
    /// Should never panic unless the EDMX `SimpleIdentifier` parser is broken.
    #[allow(clippy::unwrap_in_result)]
    pub fn redfish_resource_type(&self) -> Result<(QualifiedName<'a>, &'a EntityType), Error<'a>> {
        let ns: EdmxNamespace = "Resource".parse().expect("must be parsed");
        let id: SimpleIdentifier = "Resource".parse().expect("must be parsed");
        let schema = self
            .get(&Namespace::new(&ns))
            .ok_or(Error::ResourceTypeNotFound)?;
        let (name, _) = schema
            .entity_types
            .get_key_value(&id)
            .ok_or(Error::ResourceTypeNotFound)?;
        let qtype = QualifiedName::new(&schema.namespace, name);
        self.find_entity_type_by_qname(&qtype)
            .map(|v| (qtype, v))
            .ok_or(Error::ResourceTypeNotFound)
    }

    /// Find the `Resource.ResourceCollection` type corresponding that is base
    /// type for all Redfish resources collection
    ///
    /// # Errors
    ///
    /// Returns an error if the type is not found.
    ///
    /// # Panics
    ///
    /// Should never panic unless the EDMX `SimpleIdentifier` parser is broken.
    #[allow(clippy::unwrap_in_result)]
    pub fn redfish_resource_collection_type(
        &self,
    ) -> Result<(QualifiedName<'a>, &'a EntityType), Error<'a>> {
        let ns: EdmxNamespace = "Resource".parse().expect("must be parsed");
        let id: SimpleIdentifier = "ResourceCollection".parse().expect("must be parsed");
        let schema = self
            .get(&Namespace::new(&ns))
            .ok_or(Error::ResourceCollectionTypeNotFound)?;
        let (name, _) = schema
            .entity_types
            .get_key_value(&id)
            .ok_or(Error::ResourceCollectionTypeNotFound)?;
        let qtype = QualifiedName::new(&schema.namespace, name);
        self.find_entity_type_by_qname(&qtype)
            .map(|v| (qtype, v))
            .ok_or(Error::ResourceTypeNotFound)
    }

    #[must_use]
    fn find_entity_type_by_qname(&self, qtype: &QualifiedName<'a>) -> Option<&'a EntityType> {
        self.get(&qtype.namespace)
            .and_then(|ns| ns.entity_types.get(qtype.name))
    }

    #[must_use]
    fn find_complex_type_by_qname(&self, qtype: &QualifiedName<'a>) -> Option<&'a ComplexType> {
        self.get(&qtype.namespace)
            .and_then(|ns| ns.types.get(qtype.name))
            .and_then(|t| {
                if let Type::ComplexType(ct) = t {
                    Some(ct)
                } else {
                    None
                }
            })
    }

    fn child_adds_property(&self, qtype: &QualifiedName<'_>) -> bool {
        self.find_entity_type_by_qname(qtype)
            .map(|et| {
                !et.properties.is_empty()
                    || self.child_map.get(qtype).is_some_and(|children| {
                        children.iter().any(|child| self.child_adds_property(child))
                    })
            })
            .or_else(|| {
                self.find_complex_type_by_qname(qtype).map(|ct| {
                    !ct.properties.is_empty()
                        || self.child_map.get(qtype).is_some_and(|children| {
                            children.iter().any(|child| self.child_adds_property(child))
                        })
                })
            })
            .is_some_and(identity)
    }
}

/// Find a cycle in the `derived type -> base type` inheritance map.
///
/// Every type has at most one base, so the graph can be checked by walking
/// each inheritance chain. `positions` records where a type first appeared in
/// the current chain; seeing it again identifies the cyclic suffix. `visited`
/// contains chains already proven acyclic, preventing repeated work. The walk
/// is iterative so a deeply nested, valid hierarchy cannot overflow the stack.
/// Starting types are sorted, and the cycle is rotated to its smallest member,
/// making the reported path deterministic.
fn find_inheritance_cycle<'a>(
    base_map: &HashMap<QualifiedName<'a>, QualifiedName<'a>>,
) -> Option<Vec<QualifiedName<'a>>> {
    let mut visited = HashSet::new();
    let mut starts = base_map.keys().copied().collect::<Vec<_>>();
    starts.sort_unstable();
    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let mut path = Vec::<QualifiedName<'a>>::new();
        let mut positions = HashMap::new();
        let mut current = start;
        loop {
            // This chain joined one that was already checked successfully.
            if visited.contains(&current) {
                break;
            }
            if let Some(position) = positions.get(&current).copied() {
                let mut cycle = path[position..].to_vec();
                let canonical_start = cycle
                    .iter()
                    .enumerate()
                    .min_by(|(_, left), (_, right)| left.cmp(right))
                    .map_or(0, |(index, _)| index);
                cycle.rotate_left(canonical_start);
                // Repeat the first cyclic type to return a closed cycle path.
                if let Some(first) = cycle.first().copied() {
                    cycle.push(first);
                }
                return Some(cycle);
            }
            positions.insert(current, path.len());
            path.push(current);
            if let Some(base) = base_map.get(&current) {
                current = *base;
            } else {
                break;
            }
        }
        visited.extend(path);
    }
    None
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::edmx::Edmx;

    fn schema_with_types(types: &str) -> Vec<Edmx> {
        let schema = format!(
            r#"<edmx:Edmx Version="4.0">
                 <edmx:DataServices>
                   <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Cycle">
                     {types}
                   </Schema>
                 </edmx:DataServices>
               </edmx:Edmx>"#
        );
        vec![Edmx::parse(&schema).expect("cycle test schema must be valid")]
    }

    fn assert_cycle(result: Result<SchemaIndex<'_>, Error<'_>>, expected: &[&str]) {
        assert!(matches!(result, Err(Error::CyclicType(_))));
        if let Err(Error::CyclicType(cycle)) = result {
            assert_eq!(
                cycle.iter().map(ToString::to_string).collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn rejects_cyclic_entity_type_inheritance() {
        let schemas = schema_with_types(
            r#"<EntityType Name="B" BaseType="Cycle.A"/>
               <EntityType Name="A" BaseType="Cycle.B"/>"#,
        );

        assert_cycle(
            SchemaIndex::build(&schemas),
            &["Cycle.A", "Cycle.B", "Cycle.A"],
        );
    }

    #[test]
    fn rejects_cyclic_complex_type_inheritance() {
        let schemas = schema_with_types(
            r#"<ComplexType Name="C" BaseType="Cycle.B"/>
               <ComplexType Name="B" BaseType="Cycle.C"/>
               <ComplexType Name="A" BaseType="Cycle.C"/>"#,
        );

        assert_cycle(
            SchemaIndex::build(&schemas),
            &["Cycle.B", "Cycle.C", "Cycle.B"],
        );
    }

    #[test]
    fn rejects_self_inheritance() {
        let schemas = schema_with_types(r#"<ComplexType Name="A" BaseType="Cycle.A"/>"#);

        assert_cycle(SchemaIndex::build(&schemas), &["Cycle.A", "Cycle.A"]);
    }

    #[test]
    fn reports_first_cycle_deterministically() {
        let schemas = schema_with_types(
            r#"<EntityType Name="D" BaseType="Cycle.C"/>
               <EntityType Name="C" BaseType="Cycle.D"/>
               <EntityType Name="B" BaseType="Cycle.A"/>
               <EntityType Name="A" BaseType="Cycle.B"/>"#,
        );

        assert_cycle(
            SchemaIndex::build(&schemas),
            &["Cycle.A", "Cycle.B", "Cycle.A"],
        );
    }

    #[test]
    fn schema_index_test() {
        let schemas = [
            r#"<edmx:Edmx Version="4.0">
             <edmx:DataServices>
               <Schema Namespace="Schema.v1_0_0"/>
             </edmx:DataServices>
           </edmx:Edmx>"#,
            // Two schemas per document
            r#"<edmx:Edmx Version="4.0">
             <edmx:DataServices>
               <Schema Namespace="Schema.v1_1_0"/>
               <Schema Namespace="Schema.v1_2_0"/>
             </edmx:DataServices>
           </edmx:Edmx>"#,
        ]
        .iter()
        .map(|s| Edmx::parse(*s))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

        let index = SchemaIndex::build(&schemas).expect("acyclic schemas must be indexed");
        assert!(index
            .get(&Namespace::new(&"Schema.v1_1_0".parse().unwrap()))
            .is_some());
        assert!(index
            .get(&Namespace::new(&"Schema.v1_0_0".parse().unwrap()))
            .is_some());
        assert!(index
            .get(&Namespace::new(&"Schema.v1_2_0".parse().unwrap()))
            .is_some());
        assert!(index
            .get(&Namespace::new(&"Schema.v1_3_0".parse().unwrap()))
            .is_none());
    }
}
