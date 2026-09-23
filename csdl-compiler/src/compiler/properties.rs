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

use crate::compiler::ensure_type;
use crate::compiler::redfish::RedfishProperty;
use crate::compiler::Compiled;
use crate::compiler::ComplexType;
use crate::compiler::Context;
use crate::compiler::EntityType;
use crate::compiler::Error;
use crate::compiler::MapType;
use crate::compiler::MustHaveId;
use crate::compiler::OData;
use crate::compiler::QualifiedName;
use crate::compiler::RigidArraySupport;
use crate::compiler::Stack;
use crate::compiler::TypeClass;
use crate::edmx::property::Property as EdmxProperty;
use crate::edmx::property::PropertyAttrs;
use crate::edmx::NavigationProperty as EdmxNavigationProperty;
use crate::edmx::PropertyName;
use crate::odata::annotations::Permissions;
use crate::IsNullable;
use crate::OneOrCollection;

/// Combined structural and navigation properties.
#[derive(Default, Debug)]
pub struct Properties<'a> {
    /// Structural properties.
    pub properties: Vec<Property<'a>>,
    /// Navigation properties.
    pub nav_properties: Vec<NavProperty<'a>>,
}

impl<'a> Properties<'a> {
    /// Compile an entity/complex type's properties (structural and
    /// navigation), along with their type dependencies.
    ///
    /// # Errors
    ///
    /// Returns an error if a property or its dependency fails to compile.
    pub fn compile(
        qtype: QualifiedName<'_>,
        props: &'a [EdmxProperty],
        ctx: &Context<'a>,
        stack: Stack<'a, '_>,
    ) -> Result<(Compiled<'a>, Self), Error<'a>> {
        props
            .iter()
            .try_fold((stack, Properties::default()), |(stack, mut p), sp| {
                let stack = match &sp.attrs {
                    PropertyAttrs::StructuralProperty(v) => {
                        let resolved = ctx
                            .schema_index
                            .find_child_type(v.ptype.qualified_type_name().into());
                        // A single-valued self-reference has no
                        // representation in generated Rust — a struct cannot
                        // contain itself without indirection — so it is
                        // refused loudly here. A `Collection` edge is the
                        // representable shape; cycles through other types
                        // are refused wholesale by `ensure_type`.
                        if matches!(v.ptype, OneOrCollection::One(_))
                            && stack.nearest_complex_type() == Some(resolved)
                        {
                            return Err(Error::Property(
                                &sp.name,
                                Box::new(Error::UnrepresentableCycle(resolved)),
                            ));
                        }
                        let (compiled, typeinfo) = ensure_type(resolved, ctx, &stack)
                            .map_err(Box::new)
                            .map_err(|e| Error::Property(&sp.name, e))?;
                        p.properties.push(Property {
                            name: &v.name,
                            // Keep the property pointing at the type compiled above.
                            // The declared base may be empty and pruned later.
                            ptype: v.ptype.as_ref().map(|_| (typeinfo, resolved)),
                            odata: OData::new(MustHaveId::new(false), v),
                            redfish: RedfishProperty::new(v),
                            nullable: v.nullable.unwrap_or(IsNullable::new(true)),
                            rigid_array_support: RigidArraySupport::new(
                                ctx.config.rigid_array_filter.matches(qtype, &v.name),
                            ),
                        });
                        stack.merge(compiled)
                    }
                    PropertyAttrs::NavigationProperty(v) => {
                        let compiled = Self::compile_nav_property(&mut p, v, ctx, &stack)
                            .map_err(Box::new)
                            .map_err(|e| Error::Property(&sp.name, e))?;
                        stack.merge(compiled)
                    }
                };
                Ok((stack, p))
            })
            .map(|(stack, p)| (stack.done(), p))
    }

    /// Join properties in reverse order (from parent to child).
    /// Useful when combining properties collected across inheritance
    /// from current type and its ancestors.
    #[must_use]
    pub fn rev_join(src: Vec<Self>) -> Self {
        let (properties, nav_properties): (Vec<_>, Vec<_>) = src
            .into_iter()
            .map(|v| (v.properties, v.nav_properties))
            .unzip();
        Self {
            properties: properties.into_iter().rev().flatten().collect(),
            nav_properties: nav_properties.into_iter().rev().flatten().collect(),
        }
    }

    /// No properties defined.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.properties.is_empty() && self.nav_properties.is_empty()
    }

    fn compile_nav_property(
        p: &mut Self,
        v: &'a EdmxNavigationProperty,
        ctx: &Context<'a>,
        stack: &Stack<'a, '_>,
    ) -> Result<Compiled<'a>, Error<'a>> {
        let qname = v.ptype.qualified_type_name().into();
        let redfish = RedfishProperty::new(v);
        if ctx.root_set_entities.contains(&qname) || ctx.config.entity_type_filter.matches(&qname) {
            // Find the deepest available child in the type hierarchy
            // for the singleton, to target the most recent protocol
            // version.
            let (qtype, et) = ctx.schema_index.find_child_entity_type(qname)?;
            let (ptype, compiled) = {
                if stack.contains_entity(qtype) {
                    // Already compiled entity
                    Ok(Compiled::default())
                } else {
                    EntityType::compile(qtype, et, ctx, stack)
                        .map_err(Box::new)
                        .map_err(|e| Error::EntityType(qtype, e))
                }
                .map(|compiled| (qtype, compiled))
            }?;
            let compiled = if let Some(ec) = redfish.excerpt_copy.clone() {
                compiled.merge(Compiled::new_excerpt_copy(qtype, ec))
            } else {
                compiled
            };
            p.nav_properties
                .push(NavProperty::Expandable(NavPropertyExpandable {
                    name: &v.name,
                    ptype: v.ptype.as_ref().map(|_| ptype),
                    odata: OData::new(MustHaveId::new(false), v),
                    redfish,
                    nullable: v.nullable.unwrap_or(IsNullable::new(false)),
                }));
            Ok(compiled)
        } else {
            if redfish.excerpt_copy.is_none() {
                // Don't add excerpt copy of entities that are not
                // included in entity pattern.
                p.nav_properties.push(NavProperty::Reference {
                    cardinality: v.ptype.as_ref().map(|_| &v.name),
                    odata: OData::new(MustHaveId::new(false), v),
                    redfish,
                });
            }
            Ok(Compiled::default())
        }
    }
}

/// Additional type information used by properties.
#[derive(Clone, Copy, Debug)]
pub struct TypeInfo {
    /// Class of the type.
    pub class: TypeClass,
    /// Permissions associated with the type.
    ///
    /// In Redfish, type-level permissions are only used for two
    /// complex types (`Status` and `Condition`) in the `Resource`
    /// namespace, but supporting this is important as they sit in the
    /// base class of all Redfish resources.
    pub permissions: Option<Permissions>,
}

impl TypeInfo {
    /// Create simple type info.
    #[must_use]
    pub const fn simple_type() -> Self {
        Self {
            class: TypeClass::SimpleType,
            permissions: None,
        }
    }
    /// Create enum type info.
    #[must_use]
    pub const fn enum_type() -> Self {
        Self {
            class: TypeClass::EnumType,
            permissions: None,
        }
    }
    /// Create type definition info.
    #[must_use]
    pub const fn type_definition() -> Self {
        Self {
            class: TypeClass::TypeDefinition,
            permissions: None,
        }
    }
    /// Complex type info.
    #[must_use]
    pub fn complex_type(ct: &ComplexType) -> Self {
        let required_on_create = ct
            .properties
            .properties
            .iter()
            .any(|p| p.redfish.is_required_on_create.into_inner())
            || ct.properties.nav_properties.iter().any(|p| {
                matches!(
                    p,
                    NavProperty::Expandable(v)
                        if v.redfish.is_required_on_create.into_inner()
                )
            });

        let structural_properties_are_read_only = !ct.properties.properties.is_empty()
            && ct.properties.nav_properties.is_empty()
            && ct.properties.properties.iter().all(|p| {
                p.odata.permissions.is_some_and(|v| v == Permissions::Read)
                    || *p
                        .ptype
                        .map(|v| v.0.permissions.is_some_and(|v| v == Permissions::Read))
                        .inner()
            });

        Self {
            class: TypeClass::ComplexType,
            permissions: if required_on_create {
                // `None` means "do not treat the type itself as read-only".
                // A RequiredOnCreate member must have a serializable payload,
                // even if the same shape would otherwise look read-only.
                None
            } else {
                ct.odata.permissions.or_else(|| {
                    // Consider a complex type read-only if it has no
                    // structural or navigation properties, or if all
                    // structural properties are read-only and there are no
                    // navigation properties. RequiredOnCreate above keeps
                    // create payload shapes writable even when the type is
                    // otherwise read-only.
                    if ct.redfish.dynamic_properties.is_none()
                        && ct.odata.additional_properties.is_none_or(|v| {
                            // Redfish-specific heuristic: treat additional
                            // properties of `OemActions` complex types as
                            // read-only; we do this because the schema does not indicate their
                            // immutability.
                            !v.into_inner() || ct.name.name.inner().as_str() == "OemActions"
                        })
                        && (ct.properties.is_empty() || structural_properties_are_read_only)
                    {
                        Some(Permissions::Read)
                    } else {
                        None
                    }
                })
            },
        }
    }
}

/// Structural property type (one or a collection).
pub type PropertyType<'a> = OneOrCollection<(TypeInfo, QualifiedName<'a>)>;

impl<'a> PropertyType<'a> {
    /// Qualified type name of the property.
    #[must_use]
    pub const fn name(&self) -> QualifiedName<'a> {
        self.inner().1
    }
}

/// Structural property.
#[derive(Debug)]
pub struct Property<'a> {
    /// Property identifier.
    pub name: &'a PropertyName,
    /// Property type (one or collection).
    pub ptype: PropertyType<'a>,
    /// Attached `OData` annotations.
    pub odata: OData<'a>,
    /// Redfish-specific property annotations.
    pub redfish: RedfishProperty<'a>,
    /// Whether the property is nullable.
    pub nullable: IsNullable,
    /// Redfish specification is not very specific about which
    /// properties can be rigid and which cannot be. Rigid arrays
    /// (collections) can contain null in JSON representation. In
    /// practice only handful of properties used as rigid by BMC
    /// implementors. This flag defines
    pub rigid_array_support: RigidArraySupport,
}

impl<'a> MapType<'a> for Property<'a> {
    fn map_type<F>(mut self, f: F) -> Self
    where
        F: FnOnce(QualifiedName<'a>) -> QualifiedName<'a>,
    {
        self.ptype = self.ptype.map(|(typeclass, t)| (typeclass, f(t)));
        self
    }
}

/// Navigation property target type (one or a collection).
pub type NavPropertyType<'a> = OneOrCollection<QualifiedName<'a>>;

impl<'a> NavPropertyType<'a> {
    /// Qualified type name of the property.
    #[must_use]
    pub const fn name(&self) -> QualifiedName<'a> {
        *self.inner()
    }
}

/// Navigation property, either expandable or reference.
#[derive(Debug)]
pub enum NavProperty<'a> {
    /// Expandable navigation property (with known type).
    Expandable(NavPropertyExpandable<'a>),
    /// Reference navigation property (type is left as reference).
    Reference {
        /// Property cardinality and identifier.
        cardinality: OneOrCollection<&'a PropertyName>,
        /// Attached `OData` annotations.
        odata: OData<'a>,
        /// Redfish-specific property annotations.
        redfish: RedfishProperty<'a>,
    },
}

impl<'a> NavProperty<'a> {
    /// Name of the property regardless of variant.
    #[must_use]
    pub const fn name(&'a self) -> &'a PropertyName {
        match self {
            Self::Expandable(v) => v.name,
            Self::Reference { cardinality, .. } => cardinality.inner(),
        }
    }
}

/// Expandable navigation property details.
#[derive(Debug)]
pub struct NavPropertyExpandable<'a> {
    /// Property identifier.
    pub name: &'a PropertyName,
    /// Target type (one or collection).
    pub ptype: NavPropertyType<'a>,
    /// Attached `OData` annotations.
    pub odata: OData<'a>,
    /// Redfish-specific property annotations.
    pub redfish: RedfishProperty<'a>,
    /// Whether the property is nullable.
    pub nullable: IsNullable,
}

impl<'a> MapType<'a> for NavProperty<'a> {
    fn map_type<F>(self, f: F) -> Self
    where
        F: FnOnce(QualifiedName<'a>) -> QualifiedName<'a>,
    {
        match self {
            Self::Expandable(mut exp) => {
                exp.ptype = exp.ptype.map(f);
                Self::Expandable(exp)
            }
            Self::Reference { .. } => self,
        }
    }
}
