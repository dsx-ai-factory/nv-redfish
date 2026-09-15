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

//! Compilation output aggregate
//!
//! `Compiled` is the intermediate representation produced by the
//! compiler. It groups all resolved types and metadata in stable maps
//! keyed by fully qualified names so the Rust generator can render code
//! deterministically.
//!
//! Contents
//! - Entity and complex types with their properties and captured
//!   `OData`/Redfish annotations
//! - Enum types and type definitions
//! - Bound actions (parameters, return types) grouped by binding type
//! - A set of creatable entity types (collections that accept inserts)
//!
//! Notes
//! - Keys are `QualifiedName`s; merge operations favor later entries
//!   and union action maps.
//! - This module contains small helpers to build/merge `Compiled`
//!   fragments as the traversal proceeds.
//! - No codegen decisions happen here; the structure is intentionally
//!   straightforward for generators to consume.

use crate::compiler::annotations::Annotations;
use crate::compiler::Action;
use crate::compiler::ComplexType;
use crate::compiler::EntityType;
use crate::compiler::EnumType;
use crate::compiler::MustHaveType;
use crate::compiler::QualifiedName;
use crate::compiler::TypeDefinition;
use crate::edmx::ActionName;
use crate::redfish::ExcerptCopy;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::collections::HashSet;
use std::iter::once as iter_once;
use tagged_types::TaggedType;

/// Map from action name to compiled action.
pub type ActionsMap<'a> = HashMap<&'a ActionName, Action<'a>>;
/// All actions that belong to a type, keyed by its qualified name.
pub type TypeActions<'a> = HashMap<QualifiedName<'a>, ActionsMap<'a>>;

/// Set of all required excerpt copies of one type.
pub type ExcerptCopiesSet = HashSet<ExcerptCopy>;
/// Map from type name to required excerpt copies.
pub type ExcerptCopiesMap<'a> = HashMap<QualifiedName<'a>, ExcerptCopiesSet>;

/// Whether a type is creatable.
pub type IsCreatable = TaggedType<bool, IsCreatableTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum IsCreatableTag {}

/// Whether a type must emit an `Update` struct.
pub type ForcedUpdate = TaggedType<bool, ForcedUpdateTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum ForcedUpdateTag {}

/// Compiled outputs from schemas.
/// Aggregated compilation outputs for a set of schemas.
#[derive(Default, Debug)]
pub struct Compiled<'a> {
    /// Shared protocol annotations and their resolved schema types.
    pub annotations: Annotations<'a>,
    /// Compiled complex types by name.
    pub complex_types: HashMap<QualifiedName<'a>, ComplexType<'a>>,
    /// Compiled entity types by name.
    pub entity_types: HashMap<QualifiedName<'a>, EntityType<'a>>,
    /// Compiled type definitions by name.
    pub type_definitions: HashMap<QualifiedName<'a>, TypeDefinition<'a>>,
    /// Compiled enums by name.
    pub enum_types: HashMap<QualifiedName<'a>, EnumType<'a>>,
    /// Actions bound to each type.
    pub actions: TypeActions<'a>,
    /// Entity types whose collections are creatable.
    pub creatable_entity_types: HashSet<QualifiedName<'a>>,
    /// Excerpt copies of entity types that need to be generated.
    pub excerpt_copies: ExcerptCopiesMap<'a>,
}

impl<'a> Compiled<'a> {
    /// Create a compiled structure containing a single compiled
    /// entity type.
    #[must_use]
    pub fn new_entity_type(v: EntityType<'a>) -> Self {
        let creatable_entity_types = v
            .insertable_member_type()
            .map_or_else(HashSet::new, |insertable_type| {
                iter_once(&insertable_type).copied().collect()
            });
        Self {
            entity_types: vec![(v.name, v)].into_iter().collect(),
            creatable_entity_types,
            ..Default::default()
        }
    }

    /// Create a compiled structure containing a single compiled
    /// complex type.
    #[must_use]
    pub fn new_complex_type(v: ComplexType<'a>) -> Self {
        Self {
            complex_types: vec![(v.name, v)].into_iter().collect(),
            ..Default::default()
        }
    }

    /// Create a compiled structure containing a single type definition.
    #[must_use]
    pub fn new_type_definition(v: TypeDefinition<'a>) -> Self {
        Self {
            type_definitions: vec![(v.name, v)].into_iter().collect(),
            ..Default::default()
        }
    }

    /// Create a compiled structure containing a single enum type.
    #[must_use]
    pub fn new_enum_type(v: EnumType<'a>) -> Self {
        Self {
            enum_types: vec![(v.name, v)].into_iter().collect(),
            ..Default::default()
        }
    }

    /// Create a compiled structure containing a single action.
    #[must_use]
    pub fn new_action(v: Action<'a>) -> Self {
        Self {
            actions: vec![(v.binding, vec![(v.name, v)].into_iter().collect())]
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    /// Create a excerpt copy reference of the entity type.
    #[must_use]
    pub fn new_excerpt_copy(qtype: QualifiedName<'a>, copy: ExcerptCopy) -> Self {
        Self {
            excerpt_copies: vec![(qtype, vec![copy].into_iter().collect::<HashSet<_>>())]
                .into_iter()
                .collect(),
            ..Default::default()
        }
    }

    /// Add @data.type field marker to the type.
    #[must_use]
    pub fn mark_odata_type(mut self, qtype: QualifiedName<'a>) -> Self {
        if let Some(et) = self.entity_types.get_mut(&qtype) {
            et.odata.must_have_type = MustHaveType::new(true);
        }
        self
    }

    /// Merge two compiled structures.
    #[must_use]
    pub fn merge(mut self, other: Self) -> Self {
        self.annotations = self.annotations.merge(other.annotations);
        self.complex_types.extend(other.complex_types);
        self.type_definitions.extend(other.type_definitions);
        self.enum_types.extend(other.enum_types);
        self.entity_types.extend(other.entity_types);
        self.creatable_entity_types
            .extend(other.creatable_entity_types);
        self.actions =
            other
                .actions
                .into_iter()
                .fold(self.actions, |mut selfactions, (qname, actions)| {
                    let new_actions = match selfactions.remove(&qname) {
                        None => actions,
                        Some(mut v) => {
                            v.extend(actions);
                            v
                        }
                    };
                    selfactions.insert(qname, new_actions);
                    selfactions
                });

        for (qtype, copies) in other.excerpt_copies {
            excerpt_copies_merge_to(&mut self.excerpt_copies, qtype, copies);
        }
        self
    }

    /// Types that must emit an `Update` struct because a derived type
    /// embeds them as the base of its own `Update`.
    ///
    /// A derived `Update` embeds its base type's `Update`, so every
    /// ancestor of an updating type in the inheritance chain must also
    /// generate one -- even a read-only base under a writable
    /// derivation. Versioned Redfish schemas skip versions, so a base
    /// may be several versions back; the chain is followed by name.
    #[must_use]
    pub fn forced_updates(&self) -> HashSet<QualifiedName<'a>> {
        let mut forced = HashSet::new();
        mark_update_ancestors(
            &self.complex_types,
            ComplexType::generates_update,
            |t| t.base,
            &mut forced,
        );
        mark_update_ancestors(
            &self.entity_types,
            EntityType::generates_update,
            |t| t.base,
            &mut forced,
        );
        forced
    }
}

/// Walk each updating type's base chain and record its ancestors in
/// `forced`, stopping at an already-seen ancestor (which cuts cycles
/// and shared prefixes).
fn mark_update_ancestors<'a, T>(
    types: &HashMap<QualifiedName<'a>, T>,
    generates_update: impl Fn(&T) -> bool,
    base: impl Fn(&T) -> Option<QualifiedName<'a>>,
    forced: &mut HashSet<QualifiedName<'a>>,
) {
    for t in types.values() {
        if generates_update(t) {
            let mut next = base(t);
            while let Some(name) = next {
                if !forced.insert(name) {
                    break;
                }
                next = types.get(&name).and_then(&base);
            }
        }
    }
}

/// Merge `copies` to type with name `name` in `target`.
pub fn excerpt_copies_merge_to<'a>(
    target: &mut ExcerptCopiesMap<'a>,
    name: QualifiedName<'a>,
    copies: ExcerptCopiesSet,
) {
    match target.entry(name) {
        Entry::Occupied(mut e) => {
            e.get_mut().extend(copies);
        }
        Entry::Vacant(e) => {
            e.insert(copies);
        }
    }
}
