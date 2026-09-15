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

//! Remove empty complex types optimization.
//!
//! Compiler can remove complex types that doesn't have any properties
//! and navigation properties. Redfish schema introduces plenty of
//! such types. They are definitely not needed for code generation.

use crate::compiler::Compiled;
use crate::compiler::ComplexType;
use crate::compiler::MapBase as _;
use crate::compiler::MapType as _;
use crate::compiler::PropertiesManipulation as _;
use crate::compiler::Property;
use crate::compiler::QualifiedName;
use crate::optimizer::map_types_in_actions;
use crate::optimizer::replace;
use crate::optimizer::Config;
use std::collections::HashMap;

type Replacements<'a> = HashMap<QualifiedName<'a>, QualifiedName<'a>>;

pub fn remove_empty_complex_types<'a>(input: Compiled<'a>, _config: &Config) -> Compiled<'a> {
    let ct_replacements = collect_ct_replacements(&input);
    let map_prop = |p: Property<'a>| p.map_type(|t| replace(&t, &ct_replacements));
    Compiled {
        complex_types: input
            .complex_types
            .into_iter()
            .filter_map(|(name, v)| {
                if ct_replacements.contains_key(&name) {
                    None
                } else {
                    Some((
                        name,
                        v.map_properties(map_prop)
                            .map_base(|base| replace(&base, &ct_replacements)),
                    ))
                }
            })
            .collect(),
        entity_types: input
            .entity_types
            .into_iter()
            .map(|(name, v)| (name, v.map_properties(map_prop)))
            .collect(),
        excerpt_copies: input.excerpt_copies,
        creatable_entity_types: input.creatable_entity_types,
        enum_types: input.enum_types,
        type_definitions: input.type_definitions,
        annotations: input
            .annotations
            .map_type(|t| replace(&t, &ct_replacements)),
        actions: map_types_in_actions(input.actions, |t| replace(&t, &ct_replacements)),
    }
}

const fn ct_is_empty(ct: &ComplexType<'_>) -> bool {
    ct.properties.is_empty()
}

fn collect_ct_replacements<'a>(input: &Compiled<'a>) -> Replacements<'a> {
    input
        .complex_types
        .values()
        .filter_map(|v| {
            if ct_is_empty(v) {
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
    while let Some(ct) = input.complex_types.get(&qname) {
        if !ct_is_empty(ct) {
            return Some(qname);
        }
        if let Some(base) = ct.base {
            qname = base;
        } else {
            return None;
        }
    }
    None
}
