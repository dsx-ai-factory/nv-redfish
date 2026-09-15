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

//! If there is only one child for complex type we can remove base
//! types and move all properties of base class to child class and
//! then remove unused based classes.
//!
//! We don't want touch parent classes with multiple chlidren because
//! in code generation it may be useful to have structure that represent
//! base class and flattened in during deserialization.
//!

use crate::compiler::Compiled;
use crate::compiler::ComplexType;
use crate::compiler::MapType as _;
use crate::compiler::Properties;
use crate::compiler::PropertiesManipulation as _;
use crate::compiler::Property;
use crate::compiler::QualifiedName;
use crate::compiler::TypeInfo;
use crate::optimizer::map_types_in_actions;
use crate::optimizer::replace;
use crate::optimizer::Config;
use std::collections::HashMap;

pub fn prune_complex_type_inheritance<'a>(input: Compiled<'a>, _config: &Config) -> Compiled<'a> {
    // 1. Create parent -> child map where parent have only one child.
    let single_child_parents = input
        .complex_types
        .iter()
        .fold(
            HashMap::<QualifiedName<'a>, (QualifiedName<'a>, u64)>::new(),
            |mut v, (_, ct)| {
                if let Some(base) = ct.base {
                    v.entry(base).or_insert((ct.name, 0)).1 += 1;
                }
                v
            },
        )
        .into_iter()
        .filter_map(|(parent, (child, cnt))| {
            if cnt == 1 {
                Some((parent, child))
            } else {
                None
            }
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
        .complex_types
        .into_iter()
        .partition(|(name, _)| replacements.contains_key(name));

    // Remaining complex types
    let complex_types = retain
        .into_iter()
        // Pass all properties from single child parents to child.
        .map(|(name, v)| {
            let mut base = v.base;
            let mut odata = v.odata;
            let mut properties = vec![v.properties];
            while let Some(next_base) = base {
                if let Some(parent) = remove.remove(&next_base) {
                    // `OData.AdditionalProperties` also governs the
                    // types deriving from the one declaring it, so
                    // folding a parent away must carry the permission
                    // over. Losing it would silently drop every
                    // undeclared property the payload carries. A type
                    // stating its own permission keeps it; the nearest
                    // ancestor that states one wins otherwise.
                    if odata.additional_properties.is_none() {
                        odata.additional_properties = parent.odata.additional_properties;
                    }
                    properties.push(parent.properties);
                    base = parent.base;
                } else {
                    break;
                }
            }
            (
                name,
                ComplexType {
                    name: v.name,
                    base,
                    properties: Properties::rev_join(properties),
                    odata,
                    redfish: v.redfish,
                    is_abstract: v.is_abstract,
                },
            )
        })
        .collect::<Vec<_>>();
    // Collect new type info for all properties that points to
    // specific types:
    let complex_types_type_info = complex_types
        .iter()
        .map(|(name, v)| (*name, TypeInfo::complex_type(v)))
        .collect::<HashMap<_, _>>();
    // NOTE: that this optimization can cause potential inconsistency
    // in type info because type info has recursive behavior for
    // permissions (permissions of ComplexType depends on permissions
    // of it's properties, and further properties may be complex types
    // and depends of permissions of properties of layer
    // below). Therefore, potentialy this optmization will not mark
    // propety with proper permissions if more than one step needed to
    // propagate permissions.
    let map_prop = |p: Property<'a>| {
        // Replace type:
        let mut p = p.map_type(|t| replace(&t, &replacements));
        // Replace type info.
        if let Some(typeinfo) = complex_types_type_info.get(&p.ptype.name()) {
            p.ptype = p.ptype.map(|(_, name)| (*typeinfo, name));
        }
        p
    };
    Compiled {
        complex_types: complex_types
            .into_iter()
            .map(|(name, v)| (name, v.map_properties(map_prop)))
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
        annotations: input.annotations.map_type(|t| replace(&t, &replacements)),
        actions: map_types_in_actions(input.actions, |t| replace(&t, &replacements)),
    }
}
