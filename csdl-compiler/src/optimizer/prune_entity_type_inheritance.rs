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

//! If there is only one child for entity type and there is no key
//! we can remove base types and move all properties of base type
//! to child type and then remove unused based types.
//!
//! We don't want touch parent classes with multiple chlidren because
//! in code generation it may be useful to have structure that represent
//! base class and flattened in during deserialization.
//!

use crate::compiler::compiled::excerpt_copies_merge_to;
use crate::compiler::Compiled;
use crate::compiler::EntityType;
use crate::compiler::MapType as _;
use crate::compiler::NavProperty;
use crate::compiler::OData;
use crate::compiler::Properties;
use crate::compiler::PropertiesManipulation as _;
use crate::compiler::QualifiedName;
use crate::optimizer::map_types_in_actions;
use crate::optimizer::replace;
use crate::optimizer::Config;
use std::collections::HashMap;

pub fn prune_entity_type_inheritance<'a>(input: Compiled<'a>, config: &Config) -> Compiled<'a> {
    // 1. Create parent -> child map where parent have only one child.
    let single_child_parents = input
        .entity_types
        .iter()
        .fold(
            HashMap::<QualifiedName<'a>, (QualifiedName<'a>, u64)>::new(),
            |mut v, (_, et)| {
                if let Some(base) = et.base {
                    v.entry(base).or_insert((et.name, 0)).1 += 1;
                }
                v
            },
        )
        .into_iter()
        .filter_map(|(parent, (child, cnt))| {
            input.entity_types.get(&parent).and_then(|parent_et| {
                if !config.never_prune.matches(&parent_et.name)
                    && parent_et.key.is_none()
                    && cnt == 1
                {
                    Some((parent, child))
                } else {
                    None
                }
            })
        })
        .collect::<HashMap<_, _>>();

    // 2. Create replacement mapping: parent -> most specific child.
    let replacements = single_child_parents
        .iter()
        .map(|(parent, mut child)| {
            while let Some(next) = single_child_parents.get(child) {
                child = next;
            }
            (*parent, *child)
        })
        .collect::<HashMap<_, _>>();

    // 3. Split complex types in two groups:
    //    a. Those that need to be removed
    //    b. Those that should retain
    let (mut remove, retain): (HashMap<_, _>, HashMap<_, _>) = input
        .entity_types
        .into_iter()
        .partition(|(name, _)| replacements.contains_key(name));

    let map_nav_prop = |p: NavProperty<'a>| p.map_type(|t| replace(&t, &replacements));
    Compiled {
        entity_types: retain
            .into_iter()
            // Pass all properties from single child parents to child.
            .map(|(name, v)| {
                let mut base = v.base;
                let mut properties = vec![v.properties];
                let mut odata = v.odata;
                while let Some(next_base) = base {
                    if let Some(parent) = remove.remove(&next_base) {
                        properties.push(parent.properties);
                        base = parent.base;
                        merge_odata(&mut odata, parent.odata);
                    } else {
                        break;
                    }
                }
                (
                    name,
                    EntityType {
                        name: v.name,
                        base,
                        key: v.key,
                        properties: Properties::rev_join(properties),
                        odata,
                        is_abstract: v.is_abstract,
                    },
                )
            })
            // Replace all names that can refer to parent classes
            .map(|(name, v)| (name, v.map_nav_properties(map_nav_prop)))
            .collect(),
        creatable_entity_types: input
            .creatable_entity_types
            .into_iter()
            .map(|name| replace(&name, &replacements))
            .collect(),
        excerpt_copies: input.excerpt_copies.into_iter().fold(
            HashMap::new(),
            |mut acc, (name, copies)| {
                // Merge copies to the new name...
                let new_name = replace(&name, &replacements);
                excerpt_copies_merge_to(&mut acc, new_name, copies);
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
        annotations: input.annotations.map_type(|t| replace(&t, &replacements)),
        actions: map_types_in_actions(input.actions, |t| replace(&t, &replacements)),
    }
}

fn merge_odata<'a>(odata: &mut OData<'a>, parent_odata: OData<'a>) {
    if !odata.must_have_type.inner() {
        odata.must_have_type = parent_odata.must_have_type;
    }
    if odata.additional_properties.is_none() {
        odata.additional_properties = parent_odata.additional_properties;
    }
    if odata.description.is_none() {
        odata.description = parent_odata.description;
    }
    if odata.long_description.is_none() {
        odata.long_description = parent_odata.long_description;
    }
    if odata.insertable.is_none() {
        odata.insertable = parent_odata.insertable;
    }
    if odata.updatable.is_none() {
        odata.updatable = parent_odata.updatable;
    }
    if odata.deletable.is_none() {
        odata.deletable = parent_odata.deletable;
    }
}
