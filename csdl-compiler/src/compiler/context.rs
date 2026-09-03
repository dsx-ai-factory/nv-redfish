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

//! Immutable compilation context.

use crate::compiler::QualifiedName;
use crate::compiler::SchemaIndex;
use crate::edmx::attribute_values;
use crate::edmx::PropertyName;
use crate::edmx::SimpleIdentifier;
use serde::de::Error as DeError;
use serde::de::Visitor;
use serde::Deserialize;
use serde::Deserializer;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::str::FromStr;

/// Compilation context.
///
/// Contains immutable data passed to all functions involved in
/// compilation.
///
/// Note: see the compilation `Stack` for the mutable state.
pub struct Context<'a> {
    /// Loaded schema search index.
    pub schema_index: SchemaIndex<'a>,
    /// Compilation configuration.
    pub config: Config,
    /// Set of root entities that must be compiled.
    pub root_set_entities: HashSet<QualifiedName<'a>>,
}

/// Compilation configuration.
/// Filter and include rules applied during compilation.
#[derive(Default)]
pub struct Config {
    /// Entity type filter applied during compilation.
    pub entity_type_filter: EntityTypeFilter,
    /// Action filter applied during compilation.
    pub action_filter: ActionFilter,
    /// Array properties that should be generated as rigid.
    pub rigid_array_filter: PropertyFilter,
}

/// Action filter specified by wildcard patterns.
pub type ActionFilter = EntityTypeFilter;

/// Qualified-name pattern for an action's defining namespace and name.
pub type ActionFilterPattern = EntityTypeFilterPattern;

/// Entity type filter specified by wildcard patterns.
pub struct EntityTypeFilter {
    patterns: Vec<EntityTypeFilterPattern>,
    permissive: bool,
}

impl Default for EntityTypeFilter {
    fn default() -> Self {
        Self {
            patterns: Vec::default(),
            permissive: true,
        }
    }
}

impl EntityTypeFilter {
    /// Create a new filter from a list of patterns. If patterns are empty,
    /// matches nothing.
    #[must_use]
    pub const fn new_restrictive(patterns: Vec<EntityTypeFilterPattern>) -> Self {
        Self {
            patterns,
            permissive: false,
        }
    }
    /// Create a new filter from a list of patterns. If patterns are empty,
    /// matches anything.
    #[must_use]
    pub const fn new_permissive(patterns: Vec<EntityTypeFilterPattern>) -> Self {
        Self {
            patterns,
            permissive: true,
        }
    }

    /// Check whether the filter matches a qualified entity type name.
    #[must_use]
    pub fn matches(&self, typename: &QualifiedName<'_>) -> bool {
        if self.permissive {
            self.patterns.is_empty() || self.patterns.iter().any(|p| p.matches(typename))
        } else {
            self.patterns.iter().any(|p| p.matches(typename))
        }
    }
}

/// Qualified-name patterns.
///
/// Possible patterns:
/// `ServiceRoot.*.*` - any `EntityType` in any version of the service root
/// `SomeNamespace.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any version of namespace `SomeNamespace`
/// `*.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any version of any namespace
#[derive(Clone, Debug)]
pub struct EntityTypeFilterPattern {
    ns_ids: Vec<Option<SimpleIdentifier>>,
    names: HashSet<SimpleIdentifier>,
}

impl EntityTypeFilterPattern {
    /// Check whether this pattern matches the qualified name.
    #[must_use]
    pub fn matches(&self, typename: &QualifiedName) -> bool {
        if !self.names.is_empty() && !self.names.contains(typename.name) {
            return false;
        }
        if typename.namespace.len() != self.ns_ids.len() {
            return false;
        }
        for depth in 0..typename.namespace.len() {
            if let Some(pattern_id) = &self.ns_ids[depth] {
                if let Some(ns) = typename.namespace.get_id(depth) {
                    if pattern_id != ns {
                        return false;
                    }
                }
            }
        }
        true
    }
}

impl FromStr for EntityTypeFilterPattern {
    type Err = FilterPatternError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut ids = s.split('.').collect::<Vec<_>>();
        if let Some(name_pattern) = ids.pop() {
            let names = if name_pattern == "*" {
                HashSet::new()
            } else {
                name_pattern
                    .split('|')
                    .map(|id| {
                        id.parse::<SimpleIdentifier>()
                            .map_err(|_| FilterPatternError::InvalidIdentifier(id.into()))
                    })
                    .collect::<Result<HashSet<_>, _>>()?
            };
            let ns_ids = ids
                .into_iter()
                .map(|id| {
                    if id == "*" {
                        Ok(None)
                    } else {
                        id.parse()
                            .map(Some)
                            .map_err(|_| FilterPatternError::InvalidIdentifier(id.into()))
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Self { ns_ids, names })
        } else {
            Err(FilterPatternError::EmptyPattern)
        }
    }
}

impl<'de> Deserialize<'de> for EntityTypeFilterPattern {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct ValVisitor {}
        impl Visitor<'_> for ValVisitor {
            type Value = EntityTypeFilterPattern;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> FmtResult {
                formatter.write_str("entity filter pattern string")
            }
            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }
        de.deserialize_string(ValVisitor {})
    }
}

/// Errors that can occur while parsing filter patterns.
#[derive(Debug)]
pub enum FilterPatternError {
    /// The pattern string is empty.
    EmptyPattern,
    /// The pattern contains an invalid identifier.
    InvalidIdentifier(String),
}

impl StdError for FilterPatternError {}

impl Display for FilterPatternError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::EmptyPattern => write!(f, "empty pattern is forbidden"),
            Self::InvalidIdentifier(v) => write!(f, "invalid pattern: {v}"),
        }
    }
}

/// Property filter is aggregation of property filters patterns for
/// faster match.
#[derive(Default)]
pub struct PropertyFilter {
    search_index: HashMap<PropertyName, EntityTypeFilter>,
}

impl PropertyFilter {
    /// Create a new filter from a list of patterns.
    #[must_use]
    pub fn new(patterns: Vec<PropertyPattern>) -> Self {
        let search_index = patterns
            .into_iter()
            .map(|p| (p.property_name, p.type_filter))
            .fold(HashMap::<_, Vec<_>>::new(), |mut m, (k, v)| {
                m.entry(k).or_default().push(v);
                m
            })
            .into_iter()
            .map(|(name, vec)| (name, EntityTypeFilter::new_restrictive(vec)))
            .collect();
        Self { search_index }
    }
    /// Check if propety with pname of type with qualified name qtype
    /// matches filter.
    #[must_use]
    pub fn matches(&self, qtype: QualifiedName, pname: &PropertyName) -> bool {
        self.search_index
            .get(pname)
            .is_some_and(|f| f.matches(&qtype))
    }
}

/// Property pattern is
/// `QualifiedTypePattern/PropertyName`
/// Where
///   `QualifiedTypePattern` is `EntityTypeFilterPattern`.
#[derive(Clone, Debug)]
pub struct PropertyPattern {
    type_filter: EntityTypeFilterPattern,
    property_name: PropertyName,
}

impl FromStr for PropertyPattern {
    type Err = PropetyPatternError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((type_filter, property_name)) = s.rsplit_once('/') {
            let type_filter = type_filter.parse().map_err(Self::Err::TypeFilterPattern)?;
            let property_name = property_name.parse().map_err(Self::Err::PropertyName)?;
            Ok(Self {
                type_filter,
                property_name,
            })
        } else {
            Err(Self::Err::NoPropertyNameDefined)
        }
    }
}

impl<'de> Deserialize<'de> for PropertyPattern {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct ValVisitor {}
        impl Visitor<'_> for ValVisitor {
            type Value = PropertyPattern;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> FmtResult {
                formatter.write_str("property filter pattern string")
            }
            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }
        de.deserialize_string(ValVisitor {})
    }
}

/// Errors that can occur while parsing filter patterns.
#[derive(Debug)]
pub enum PropetyPatternError {
    /// No property name defined in pattern.
    NoPropertyNameDefined,
    /// Type pattern error.
    TypeFilterPattern(FilterPatternError),
    /// Property name error.
    PropertyName(attribute_values::Error),
}

impl StdError for PropetyPatternError {}

impl Display for PropetyPatternError {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::NoPropertyNameDefined => write!(f, "no property name defined"),
            Self::TypeFilterPattern(v) => write!(f, "type filter error: {v}"),
            Self::PropertyName(v) => write!(f, "property name error: {v}"),
        }
    }
}
