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

//! Remove empty entity types optimization
//!
//! Compiler can remove enity types that doesn't have any properties
//! and navigation properties and key. Redfish schema introduces
//! plenty of such types. They are definitely not needed for code
//! generation.

use crate::compiler::Compiled;
use crate::compiler::EntityType;
use crate::compiler::MapBase as _;
use crate::compiler::MapType as _;
use crate::compiler::NavProperty;
use crate::compiler::PropertiesManipulation as _;
use crate::compiler::QualifiedName;
use crate::optimizer::map_types_in_actions;
use crate::optimizer::replace;
use crate::optimizer::Config;
use crate::optimizer::Replacements;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

pub fn remove_empty_entity_types<'a>(input: Compiled<'a>, config: &Config) -> Compiled<'a> {
    let et_replacements = collect_et_replacements(&input, config);
    let map_nav_prop = |p: NavProperty<'a>| p.map_type(|t| replace(&t, &et_replacements));
    Compiled {
        entity_types: input
            .entity_types
            .into_iter()
            .filter_map(|(name, v)| {
                if et_replacements.contains_key(&name) {
                    None
                } else {
                    Some((
                        name,
                        v.map_nav_properties(map_nav_prop)
                            .map_base(|base| replace(&base, &et_replacements)),
                    ))
                }
            })
            .collect(),
        creatable_entity_types: input
            .creatable_entity_types
            .into_iter()
            .map(|name| replace(&name, &et_replacements))
            .collect(),
        excerpt_copies: input.excerpt_copies.into_iter().fold(
            HashMap::new(),
            |mut acc, (name, copies)| {
                // Merge copies to the new name...
                let new_name = replace(&name, &et_replacements);
                match acc.entry(new_name) {
                    Entry::Occupied(mut e) => {
                        e.get_mut().extend(copies);
                    }
                    Entry::Vacant(e) => {
                        e.insert(copies);
                    }
                }
                acc
            },
        ),
        complex_types: input
            .complex_types
            .into_iter()
            .map(|(name, v)| (name, v.map_nav_properties(map_nav_prop)))
            .collect(),
        enum_types: input.enum_types,
        type_definitions: input.type_definitions,
        annotations: input
            .annotations
            .map_type(|t| replace(&t, &et_replacements)),
        actions: map_types_in_actions(input.actions, |t| replace(&t, &et_replacements)),
    }
}

const fn et_is_empty(et: &EntityType<'_>) -> bool {
    et.properties.is_empty() && et.key.is_none() && et.odata.is_empty()
}

fn collect_et_replacements<'a>(input: &Compiled<'a>, config: &Config) -> Replacements<'a> {
    input
        .entity_types
        .values()
        .filter_map(|v| {
            if !config.never_prune.matches(&v.name) && et_is_empty(v) {
                find_non_empty_parent(input, v.name).map(|parent| (v.name, parent))
            } else {
                None
            }
        })
        .collect()
}

fn find_non_empty_parent<'a>(
    input: &Compiled<'a>,
    mut qname: QualifiedName<'a>,
) -> Option<QualifiedName<'a>> {
    while let Some(et) = input.entity_types.get(&qname) {
        if !et_is_empty(et) {
            return Some(qname);
        }
        if let Some(base) = et.base {
            qname = base;
        } else {
            return None;
        }
    }
    None
}
