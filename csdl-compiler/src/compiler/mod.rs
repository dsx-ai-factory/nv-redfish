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

//! Schema compiler pipeline
//!
//! This module turns a set of EDMX schemas into an intermediate
//! representation (`Compiled`) that is later consumed by the Rust
//! generator. The flow is intentionally simple and predictable:
//!
//! 1) Index
//!    - Build a `SchemaIndex` across all EDMX documents to resolve
//!      names and follow inheritance chains (entity/complex types).
//!
//! 2) Root set
//!    - Choose what to compile: either a root set derived from service
//!      singletons (`compile`) or all entity/complex types
//!      (`compile_all`). The `Config`/`EntityTypeFilter` can narrow
//!      which navigation targets are pulled in.
//!
//! 3) Traverse and compile
//!    - Walk entity and complex types, compiling structural and
//!      navigation properties. Navigation properties resolve to the
//!      most specific descendant type that adds properties, allowing
//!      newer protocol versions to be targeted.
//!    - A `Stack` tracks frames and prevents cycles when types refer to
//!      each other via navigation properties.
//!    - `OData` and Redfish-specific annotations are captured alongside
//!      types for later codegen (permissions, insert/update/delete,
//!      required flags, etc.).
//!
//! 4) Actions
//!    - Compile bound actions, their parameters and return types, and
//!      attach them to the binding type in `Compiled`.
//!
//! Output
//! - The result is a `Compiled` aggregate containing maps of entity
//!   types, complex types, enums, type definitions, and actions. It is
//!   designed to be stable, readable, and straightforward for the
//!   generator to consume.

#![deny(missing_docs)]

/// Compiled action.
pub mod action;
/// Compiled schema bundle.
pub mod compiled;
/// Compiled complex type.
pub mod complex_type;
/// Compilation context.
pub mod context;
/// Compiled entity type.
pub mod entity_type;
/// Compiled enum type.
pub mod enum_type;
/// Error diagnostics.
pub mod error;
/// Compiled namespace.
pub mod namespace;
/// Compiled OData.
pub mod odata;
/// Compiled action parameter.
pub mod parameter;
/// Compiled properties of `ComplexType` or `EntityType`.
pub mod properties;
/// Qualified (namespace + name) type identifier.
pub mod qualified_name;
/// Compiled Redfish-specific attributes.
pub mod redfish;
/// Index over parsed schemas.
pub mod schema_index;
/// Compilation stack.
pub mod stack;
/// Traits useful during compilation.
pub mod traits;
/// Compiled type definition.
pub mod type_definition;

// Type re-exports
#[doc(inline)]
pub use action::Action;
#[doc(inline)]
pub use compiled::ActionsMap;
#[doc(inline)]
pub use compiled::Compiled;
#[doc(inline)]
pub use compiled::ForcedUpdate;

pub use compiled::IsCreatable;
#[doc(inline)]
pub use compiled::TypeActions;
#[doc(inline)]
pub use complex_type::ComplexType;
#[doc(inline)]
pub use context::ActionFilter;
#[doc(inline)]
pub use context::ActionFilterPattern;
#[doc(inline)]
pub use context::Config;
#[doc(inline)]
pub use context::Context;
#[doc(inline)]
pub use context::EntityTypeFilter;
#[doc(inline)]
pub use context::EntityTypeFilterPattern;
#[doc(inline)]
pub use context::PropertyFilter;
#[doc(inline)]
pub use context::PropertyPattern;
#[doc(inline)]
pub use entity_type::EntityType;
#[doc(inline)]
pub use enum_type::EnumType;
#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use namespace::Namespace;
#[doc(inline)]
pub use odata::OData;
#[doc(inline)]
pub use parameter::Parameter;
#[doc(inline)]
pub use parameter::ParameterType;
#[doc(inline)]
pub use properties::NavProperty;
#[doc(inline)]
pub use properties::NavPropertyExpandable;
#[doc(inline)]
pub use properties::NavPropertyType;
#[doc(inline)]
pub use properties::Properties;
#[doc(inline)]
pub use properties::Property;
#[doc(inline)]
pub use properties::PropertyType;
#[doc(inline)]
pub use properties::TypeInfo;
#[doc(inline)]
pub use qualified_name::QualifiedName;
#[doc(inline)]
pub use redfish::Redfish;
#[doc(inline)]
pub use type_definition::TypeDefinition;

// Trait re-exports
#[doc(inline)]
pub use traits::MapBase;
#[doc(inline)]
pub use traits::MapType;
#[doc(inline)]
pub use traits::PropertiesManipulation;

use crate::compiler::odata::MustHaveId;
use crate::compiler::odata::MustHaveType;
use crate::edmx::Action as EdmxAction;
use crate::edmx::Edmx;
use crate::edmx::Schema;
use crate::edmx::SimpleIdentifier;
use crate::edmx::Type;
use schema_index::SchemaIndex;
use stack::Stack;
use tagged_types::TaggedType;

/// Support of Rigid Arrays.
///
/// Redfish specification is not very specific about which properties
/// can be rigid and which cannot be. Rigid arrays (collections) can
/// contain null in JSON representation. In practice only handful of
/// properties used as rigid by BMC implementors. This flag defined
/// per property basis and provided to compiler as config.
pub type RigidArraySupport = TaggedType<bool, RigidArraySupportTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Copy, Clone)]
#[transparent(Debug, Deserialize)]
#[capability(inner_access)]
pub enum RigidArraySupportTag {}

/// Type class for property attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeClass {
    /// Simple type like `Edm.String`, `Edm.Int64` etc.
    SimpleType,
    /// Enumeration type.
    EnumType,
    /// Type definition.
    TypeDefinition,
    /// Complex type.
    ComplexType,
}

/// Collection of EDMX documents compiled together to produce code.
///
/// Root documents are compiled: their types, and for `compile_all` their
/// actions, become part of the output. Resolve documents exist only so a
/// root document can reference types defined elsewhere; those types are
/// pulled in on demand but never treated as compilation roots themselves.
pub struct SchemaBundle {
    /// Parsed and validated root documents.
    root_docs: Vec<Edmx>,

    /// Parsed and validated resolve documents.
    resolve_docs: Vec<Edmx>,
}

/// Set of types that need to be compiled.
#[derive(Debug)]
pub struct RootSet<'a> {
    entity_types: Vec<QualifiedName<'a>>,
    complex_types: Vec<QualifiedName<'a>>,
}

impl SchemaBundle {
    /// Build a bundle from root documents and, optionally, resolve
    /// documents used only to resolve type references from the root
    /// documents; resolve documents are never compiled themselves.
    #[must_use]
    pub const fn new(root_docs: Vec<Edmx>, resolve_docs: Vec<Edmx>) -> Self {
        Self {
            root_docs,
            resolve_docs,
        }
    }

    /// All documents, root and resolve, for type-reference resolution.
    fn all_docs(&self) -> impl Iterator<Item = &Edmx> + Clone {
        self.root_docs.iter().chain(&self.resolve_docs)
    }

    /// Compile multiple schemas, resolving all type dependencies.
    ///
    /// The root set is defined by the specified singletons.
    ///
    /// # Errors
    ///
    /// Returns a compile error if any type cannot be resolved.
    pub fn compile(
        &self,
        singletons: &[SimpleIdentifier],
        root_patterns: &EntityTypeFilter,
        config: Config,
    ) -> Result<Compiled<'_>, Error<'_>> {
        let schema_index = SchemaIndex::build(self.all_docs())?;
        let root_set = self.root_set_from_singletons(&schema_index, singletons, root_patterns)?;
        let ctx = Context {
            schema_index,
            config,
            root_set_entities: root_set.entity_types.iter().copied().collect(),
        };
        self.compile_root_set(&root_set, &ctx)
    }

    /// Compile multiple schemas, resolving all type dependencies.
    ///
    /// The root set includes all entity and complex types from root documents,
    /// plus the binding types of selected actions.
    ///
    /// # Errors
    ///
    /// Returns a compile error if any type cannot be resolved.
    pub fn compile_all(&self, config: Config) -> Result<Compiled<'_>, Error<'_>> {
        let root_set = self.root_set_all(&config.action_filter);

        let ctx = Context {
            schema_index: SchemaIndex::build(self.all_docs())?,
            config,
            root_set_entities: root_set.entity_types.iter().copied().collect(),
        };
        self.compile_root_set(&root_set, &ctx)
    }

    fn root_set_from_singletons<'a>(
        &'a self,
        schema_index: &SchemaIndex<'a>,
        singletons: &[SimpleIdentifier],
        root_patterns: &EntityTypeFilter,
    ) -> Result<RootSet<'a>, Error<'a>> {
        // Iterate through all singletons located in
        // EDMX documents -> schemas -> entity containers.
        //
        // If a singleton matches the requested set, collect its most recent
        // descendant type into the root set.
        let entity_types = self
            .root_docs
            .iter()
            .flat_map(|edmx| {
                edmx.data_services.schemas.iter().flat_map(|s| {
                    s.entity_container
                        .as_ref()
                        .map_or(Vec::new(), |entity_container| {
                            entity_container
                                .singletons
                                .iter()
                                .filter_map(|singleton| {
                                    if singletons.contains(&singleton.name) {
                                        Some(
                                            schema_index
                                                .find_child_entity_type((&singleton.stype).into())
                                                .map(|(qname, _)| qname),
                                        )
                                    } else {
                                        None
                                    }
                                })
                                .collect::<Vec<_>>()
                        })
                })
            })
            .chain(self.root_docs.iter().flat_map(|edmx| {
                edmx.data_services
                    .schemas
                    .iter()
                    .flat_map(|s| {
                        s.entity_types
                            .values()
                            .filter_map(|t| {
                                let name = QualifiedName::new(&s.namespace, t.name.inner());
                                if root_patterns.matches(&name) {
                                    Some(Ok(name))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            }))
            .collect::<Result<Vec<_>, _>>()?;

        let complex_types = self
            .root_docs
            .iter()
            .flat_map(|edmx| {
                edmx.data_services.schemas.iter().flat_map(|s| {
                    s.types.values().filter_map(move |t| {
                        if let Type::ComplexType(t) = t {
                            let name = QualifiedName::new(&s.namespace, t.name.inner());
                            if root_patterns.matches(&name) {
                                Some(name)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                })
            })
            .collect();

        Ok(RootSet {
            entity_types,
            complex_types,
        })
    }

    fn root_set_all(&self, action_filter: &ActionFilter) -> RootSet<'_> {
        let mut entity_types: Vec<_> = self
            .root_docs
            .iter()
            .flat_map(|edmx| {
                edmx.data_services
                    .schemas
                    .iter()
                    .flat_map(|s| {
                        s.entity_types
                            .values()
                            .map(|t| QualifiedName::new(&s.namespace, t.name.inner()))
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        let mut complex_types: Vec<_> = self
            .root_docs
            .iter()
            .flat_map(|edmx| {
                edmx.data_services
                    .schemas
                    .iter()
                    .flat_map(|s| {
                        s.types
                            .values()
                            .filter_map(|t| {
                                if let Type::ComplexType(t) = t {
                                    Some(QualifiedName::new(&s.namespace, t.name.inner()))
                                } else {
                                    None
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        // A selected action is a compilation root, so its binding type is a
        // root as well. This is primarily needed by OEM actions bound to a
        // standard OemActions type supplied in a resolution-only document.
        complex_types.extend(
            self.root_docs
                .iter()
                .flat_map(|edmx| edmx.data_services.schemas.iter())
                .flat_map(|schema| {
                    schema
                        .actions
                        .iter()
                        .filter(move |action| {
                            action.is_bound.into_inner()
                                && Self::action_matches_filter(schema, action, action_filter)
                        })
                        .filter_map(|action| action.parameters.first())
                        .map(|binding| QualifiedName::from(binding.ptype.qualified_type_name()))
                }),
        );

        // Schemas store types in hash maps, and iteration order must not
        // decide compile order: which member of a reference cycle sees
        // the provisional type info — and with it generated output —
        // would otherwise vary run to run.
        entity_types.sort_unstable();
        complex_types.sort_unstable();
        complex_types.dedup();
        RootSet {
            entity_types,
            complex_types,
        }
    }

    fn compile_root_set<'a>(
        &'a self,
        root_set: &RootSet<'a>,
        ctx: &Context<'a>,
    ) -> Result<Compiled<'a>, Error<'a>> {
        let stack = Stack::default();
        let stack = root_set
            .entity_types
            .iter()
            .try_fold(stack, |cstack, qname| {
                EntityType::ensure(*qname, ctx, &cstack).map(|compiled| cstack.merge(compiled))
            })?;
        let stack = root_set.complex_types.iter().try_fold(stack, |cstack, t| {
            ensure_type(*t, ctx, &cstack).map(|(compiled, _)| cstack.merge(compiled))
        })?;
        // Compile type for @Redfish.Settings
        let (name, _) = ctx.schema_index.redfish_settings_type()?;
        let (compiled, _) = ensure_type(name, ctx, &stack)?;
        let stack = stack.merge(compiled);
        // Compile type for @Redfish.SettingsApplyTime
        let (name, _) = ctx
            .schema_index
            .redfish_settings_preferred_apply_time_type()?;
        let (compiled, _) = ensure_type(name, ctx, &stack)?;
        let stack = stack.merge(compiled);

        let (resource_name, _) = ctx.schema_index.redfish_resource_type()?;
        let compiled = EntityType::ensure(resource_name, ctx, &stack)?;
        let stack = stack.merge(compiled);

        let (collection_name, _) = ctx.schema_index.redfish_resource_collection_type()?;
        let compiled = EntityType::ensure(collection_name, ctx, &stack)?;
        let stack = stack.merge(compiled);

        // Compile actions for all root-document types
        self.root_docs
            .iter()
            .try_fold(stack, |stack, edmx| {
                let cstack = stack.new_frame();
                let compiled = edmx
                    .data_services
                    .schemas
                    .iter()
                    .try_fold(cstack, |stack, s| {
                        Self::compile_schema_actions(s, ctx, stack.new_frame())
                            .map(|v| stack.merge(v))
                    })?
                    .done();
                Ok(stack.merge(compiled))
            })
            .map(|stack| {
                stack
                    .done()
                    .mark_odata_type(resource_name)
                    .mark_odata_type(collection_name)
            })
    }

    fn compile_schema_actions<'a>(
        s: &'a Schema,
        ctx: &Context<'a>,
        stack: Stack<'a, '_>,
    ) -> Result<Compiled<'a>, Error<'a>> {
        s.actions
            .iter()
            .filter(|action| Self::action_matches_filter(s, action, &ctx.config.action_filter))
            .try_fold(stack, |stack, action| {
                let compiled =
                    action::compile_action(action, Namespace::new(&s.namespace), ctx, &stack)
                        .map_err(Box::new)
                        .map_err(|e| Error::Action(&action.name, e))?;
                Ok(stack.merge(compiled))
            })
            .map_err(Box::new)
            .map_err(|e| Error::Schema(&s.namespace, e))
            .map(Stack::done)
    }

    fn action_matches_filter(
        schema: &Schema,
        action: &EdmxAction,
        action_filter: &ActionFilter,
    ) -> bool {
        action_filter.matches(&QualifiedName::new(&schema.namespace, action.name.inner()))
    }
}

fn is_simple_type(qtype: QualifiedName<'_>) -> bool {
    qtype.namespace.is_edm()
}

fn ensure_type<'a>(
    qtype: QualifiedName<'a>,
    ctx: &Context<'a>,
    stack: &Stack<'a, '_>,
) -> Result<(Compiled<'a>, TypeInfo), Error<'a>> {
    if is_simple_type(qtype) {
        Ok((Compiled::default(), TypeInfo::simple_type()))
    } else if let Some(info) = stack.complex_type_info(qtype) {
        Ok((Compiled::default(), info))
    } else if stack.compiling_complex_type(qtype) {
        if stack.nearest_complex_type() == Some(qtype) {
            // A directly self-referential complex type sees the type it is
            // inside of rather than compiling it again forever. The shipped
            // shape is `AttributeRegistry.v1_5_0.MapFrom`, whose
            // `Subexpressions` declares `Collection(v1_0_0.MapFrom)` and
            // becomes self-referential only when `find_child_type` resolves
            // the base version down to v1_5_0 — the guard must therefore
            // match the resolved name, never the declared one. Provisional
            // permissions stay `None`, the permissive reading: the type is
            // never classified read-only on account of a self-reference, so
            // its own Update struct is generated rather than silently
            // dropped — and because the reference is to itself, that struct
            // always exists.
            Ok((
                Compiled::default(),
                TypeInfo {
                    class: TypeClass::ComplexType,
                    permissions: None,
                },
            ))
        } else {
            // A cycle spanning more than one complex type — through
            // properties, a base, or both — would bake one member's
            // provisional info into another's compiled form: dangling
            // Update references, order-dependent output, and by-value
            // base embedding the generator has no layout for. No shipped
            // schema declares one; refuse loudly instead of emitting
            // Rust that cannot compile.
            Err(Error::UnsupportedCycle(qtype))
        }
    } else if stack.contains_type_definition(qtype) {
        Ok((Compiled::default(), TypeInfo::type_definition()))
    } else if stack.contains_enum_type(qtype) {
        Ok((Compiled::default(), TypeInfo::enum_type()))
    } else {
        compile_type(qtype, ctx, stack)
    }
}

fn compile_type<'a>(
    qtype: QualifiedName<'a>,
    ctx: &Context<'a>,
    stack: &Stack<'a, '_>,
) -> Result<(Compiled<'a>, TypeInfo), Error<'a>> {
    ctx.schema_index
        .find_type(qtype)
        .ok_or(Error::TypeNotFound(qtype))
        .and_then(|t| match t {
            Type::TypeDefinition(td) => type_definition::compile(qtype, td),
            Type::EnumType(et) => Ok(enum_type::compile(qtype, et)),
            Type::ComplexType(ct) => complex_type::compile(qtype, ct, ctx, stack),
        })
        .map_err(Box::new)
        .map_err(|e| Error::Type(qtype, e))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::edmx::Edmx;
    use crate::edmx::QualifiedTypeName;

    #[test]
    fn compile_all_propagates_cyclic_type_error() {
        let schema = r#"<edmx:Edmx Version="4.0">
             <edmx:DataServices>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Cycle">
                 <EntityType Name="A" BaseType="Cycle.B"/>
                 <EntityType Name="B" BaseType="Cycle.A"/>
               </Schema>
             </edmx:DataServices>
           </edmx:Edmx>"#;

        let bundle = SchemaBundle::new(
            vec![Edmx::parse(schema).expect("entity cycle schema must be valid")],
            Vec::new(),
        );

        let result = bundle.compile_all(Config::default());
        assert!(
            matches!(
                result,
                Err(Error::CyclicType(cycle))
                    if cycle.len() >= 2 && cycle.first() == cycle.last()
            ),
            "compile_all must propagate the closed inheritance cycle"
        );
    }

    /// `compile_all` unconditionally compiles the Redfish framework
    /// types, so every fixture carries minimal Resource and Settings
    /// declarations beside the fragment under test.
    fn scaffolded(fragment: &str) -> SchemaBundle {
        let schema = format!(
            r#"<edmx:Edmx Version="4.0">
             <edmx:DataServices>
               {fragment}
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Resource">
                 <EntityType Name="Resource" Abstract="true"/>
                 <EntityType Name="ResourceCollection" Abstract="true"/>
               </Schema>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Settings">
                 <ComplexType Name="Settings"/>
                 <ComplexType Name="PreferredApplyTime"/>
               </Schema>
             </edmx:DataServices>
           </edmx:Edmx>"#
        );

        SchemaBundle::new(
            vec![Edmx::parse(&schema).expect("fixture schema must be valid")],
            Vec::new(),
        )
    }

    fn has_complex_type(compiled: &Compiled<'_>, name: &str) -> bool {
        let qname: QualifiedTypeName = name.parse().expect("a qualified type name");
        compiled.complex_types.contains_key(&(&qname).into())
    }

    #[test]
    fn a_self_referential_complex_type_compiles() {
        // A collection of the enclosing type terminates: the guard hands
        // the self-referential property provisional type info instead of
        // compiling its own type forever.
        let bundle = scaffolded(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Registry">
                 <EntityType Name="Registry">
                   <Property Name="Dependency" Type="Registry.MapFrom" Nullable="false"/>
                 </EntityType>
                 <ComplexType Name="MapFrom">
                   <Property Name="Subexpressions" Type="Collection(Registry.MapFrom)" Nullable="false"/>
                 </ComplexType>
               </Schema>"#,
        );
        let compiled = bundle
            .compile_all(Config::default())
            .expect("a self-referential complex type terminates");
        let qname: QualifiedTypeName = "Registry.MapFrom".parse().expect("a qualified type name");
        let map_from = compiled
            .complex_types
            .get(&(&qname).into())
            .expect("MapFrom compiled");
        // The provisional info pins the permissive reading: the enclosing
        // type is never classified read-only on account of its
        // self-reference, so an Update struct is generated for it.
        assert_eq!(TypeInfo::complex_type(map_from).permissions, None);
        assert!(map_from.generates_update());
    }

    #[test]
    fn a_derived_type_self_referential_through_its_base_compiles() {
        // The shipped AttributeRegistry v1.5.0 shape: `Subexpressions`
        // declares the BASE version's name, and only becomes
        // self-referential when `find_child_type` resolves it down to the
        // derived type. A guard matching the declared name instead of the
        // resolved one would hang on exactly this fixture.
        let bundle = scaffolded(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Registry">
                 <EntityType Name="Registry">
                   <Property Name="Dependency" Type="Registry.v1_0_0.MapFrom" Nullable="false"/>
                 </EntityType>
               </Schema>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Registry.v1_0_0">
                 <ComplexType Name="MapFrom">
                   <Property Name="Tag" Type="Edm.String" Nullable="false"/>
                 </ComplexType>
               </Schema>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Registry.v1_5_0">
                 <ComplexType Name="MapFrom" BaseType="Registry.v1_0_0.MapFrom">
                   <Property Name="Subexpressions" Type="Collection(Registry.v1_0_0.MapFrom)" Nullable="false"/>
                 </ComplexType>
               </Schema>"#,
        );
        let compiled = bundle
            .compile_all(Config::default())
            .expect("the base-declared self-reference terminates");
        assert!(has_complex_type(&compiled, "Registry.v1_5_0.MapFrom"));
    }

    #[test]
    fn a_cycle_spanning_two_complex_types_is_refused() {
        // Even collection-valued: a wider cycle would bake one member's
        // provisional type info into the other's compiled form, and which
        // member that is would follow compile order.
        let bundle = scaffolded(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Pair">
                 <EntityType Name="Pair">
                   <Property Name="First" Type="Pair.A" Nullable="false"/>
                 </EntityType>
                 <ComplexType Name="A">
                   <Property Name="Others" Type="Collection(Pair.B)" Nullable="false"/>
                 </ComplexType>
                 <ComplexType Name="B">
                   <Property Name="Others" Type="Collection(Pair.A)" Nullable="false"/>
                 </ComplexType>
               </Schema>"#,
        );
        let error = bundle
            .compile_all(Config::default())
            .expect_err("a two-type cycle is refused");
        assert!(
            format!("{error:?}").contains("UnsupportedCycle"),
            "the error names the unsupported cycle: {:?}",
            error
        );
    }

    #[test]
    fn a_cycle_closed_through_a_base_type_is_refused() {
        // The by-value base edge closes the cycle here: B embeds A as its
        // base while A holds B. Inheritance-cycle detection cannot see it
        // (B -> A is not an inheritance loop), so the in-progress guard
        // must.
        let bundle = scaffolded(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Pair">
                 <EntityType Name="Pair">
                   <Property Name="First" Type="Pair.A" Nullable="false"/>
                 </EntityType>
                 <ComplexType Name="A">
                   <Property Name="Other" Type="Pair.B" Nullable="false"/>
                 </ComplexType>
                 <ComplexType Name="B" BaseType="Pair.A"/>
               </Schema>"#,
        );
        let error = bundle
            .compile_all(Config::default())
            .expect_err("a base-closed cycle is refused");
        assert!(
            format!("{error:?}").contains("UnsupportedCycle"),
            "the error names the unsupported cycle: {:?}",
            error
        );
    }

    #[test]
    fn a_single_valued_self_reference_is_refused() {
        // A struct cannot contain itself without indirection, and the
        // generator has none: only a collection-valued self-reference has
        // a representation.
        let bundle = scaffolded(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Pair">
                 <EntityType Name="Pair">
                   <Property Name="First" Type="Pair.A" Nullable="false"/>
                 </EntityType>
                 <ComplexType Name="A">
                   <Property Name="Inner" Type="Pair.A" Nullable="false"/>
                 </ComplexType>
               </Schema>"#,
        );
        let error = bundle
            .compile_all(Config::default())
            .expect_err("a single-valued self-reference is refused");
        assert!(
            format!("{error:?}").contains("UnrepresentableCycle"),
            "the error names the unrepresentable self-reference: {:?}",
            error
        );
    }

    #[test]
    fn a_complex_type_with_a_simple_base_is_refused() {
        // `ensure_type` short-circuits `Edm.*`, so the base check keeps
        // an invalid base a schema-time error rather than an unresolved
        // path in the generated crate.
        let bundle = scaffolded(
            r#"<Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Bad">
                 <EntityType Name="Bad">
                   <Property Name="Value" Type="Bad.X" Nullable="false"/>
                 </EntityType>
                 <ComplexType Name="X" BaseType="Edm.String"/>
               </Schema>"#,
        );
        let error = bundle
            .compile_all(Config::default())
            .expect_err("a simple-type base is refused");
        assert!(
            format!("{error:?}").contains("TypeNotFound"),
            "the error names the missing base: {:?}",
            error
        );
    }

    #[test]
    fn schema_test() {
        let schema = r#"<edmx:Edmx Version="4.0">
             <edmx:DataServices>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Resource">
                 <EntityType Name="ItemOrCollection" Abstract="true"/>
                 <EntityType Name="Item" BaseType="Resource.ItemOrCollection" Abstract="true"/>
                 <EntityType Name="Resource" BaseType="Resource.Item" Abstract="true"/>
                 <EntityType Name="ResourceCollection" BaseType="Resource.ItemOrCollection" Abstract="true"/>
               </Schema>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="Resource.v1_0_0">
                 <EntityType Name="Resource" BaseType="Resource.Resource" Abstract="true">
                   <Key><PropertyRef Name="Id"/></Key>
                 </EntityType>
               </Schema>
               <Schema xmlns="http://docs.oasis-open.org/odata/ns/edm" Namespace="ServiceRoot">
                 <EntityType Name="ServiceRoot" BaseType="Resource.v1_0_0.Resource" Abstract="true">
                   <Property Name="RedfishVersion" Type="Edm.String" Nullable="false">
                     <Annotation Term="OData.Description" String="The version of the Redfish service."/>
                   </Property>
                   <NavigationProperty Name="LegacyTarget" Type="ServiceRoot.Target">
                     <Annotation Term="Redfish.Deprecated" String="Use TargetV2."/>
                   </NavigationProperty>
                 </EntityType>
                 <EntityType Name="Target"/>
               </Schema>
               <Schema Namespace="Schema.v1_0_0">
                 <EntityContainer Name="ServiceContainer">
                   <Singleton Name="Service" Type="ServiceRoot.ServiceRoot"/>
                 </EntityContainer>
                 <EntityType Name="ServiceRoot" BaseType="ServiceRoot.ServiceRoot"/>
               </Schema>
               <Schema Namespace="Settings">
                 <ComplexType Name="Settings"/>
                 <ComplexType Name="PreferredApplyTime"/>
               </Schema>
             </edmx:DataServices>
           </edmx:Edmx>"#;

        let bundle = SchemaBundle::new(vec![Edmx::parse(schema).unwrap()], Vec::new());

        let compiled = bundle
            .compile(
                &["Service".parse().unwrap()],
                &EntityTypeFilter::new_restrictive(vec![]),
                Config {
                    entity_type_filter: EntityTypeFilter::new_restrictive(vec![]),
                    ..Config::default()
                },
            )
            .unwrap();
        let qtypename: QualifiedTypeName = "ServiceRoot.ServiceRoot".parse().unwrap();
        let root_type: QualifiedName<'_> = (&qtypename).into();
        let mut cur_type = &root_type;
        loop {
            let et = compiled.entity_types.get(cur_type).unwrap();
            cur_type = if let Some(t) = &et.base { t } else { break };
        }
        let qtype: QualifiedTypeName = "ServiceRoot.ServiceRoot".parse().unwrap();
        let et = compiled.entity_types.get(&(&qtype).into()).unwrap();
        assert_eq!(et.properties.properties.len(), 1);
        assert_eq!(
            et.properties.properties[0]
                .odata
                .description
                .as_ref()
                .unwrap()
                .inner(),
            &"The version of the Redfish service."
        );
        assert!(matches!(
            &et.properties.nav_properties[0],
            NavProperty::Reference { redfish, .. }
                if redfish.deprecation.is_some_and(|deprecation|
                    deprecation.description == Some("Use TargetV2."))
        ));
    }
}
