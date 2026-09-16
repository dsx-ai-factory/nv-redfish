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

//! Prune simple types namespaces.

use crate::compiler::Compiled;
use crate::compiler::MapType as _;
use crate::compiler::PropertiesManipulation as _;
use crate::compiler::Property;
use crate::optimizer::map_types_in_actions;
use crate::optimizer::replace;
use crate::optimizer::Config;

pub fn prune<'a>(input: Compiled<'a>, _config: &Config) -> Compiled<'a> {
    let replacements =
        super::prune_namepaces_replacements(|| input.type_definitions.keys().copied());
    let map_prop = |p: Property<'a>| p.map_type(|t| replace(&t, &replacements));
    Compiled {
        type_definitions: input
            .type_definitions
            .into_iter()
            .collect::<Vec<_>>()
            .into_iter()
            .map(|(name, mut t)| {
                let new_name = *replacements.get(&name).map_or(&name, |v| v);
                t.name = new_name;
                (new_name, t)
            })
            .collect(),
        enum_types: input.enum_types,
        complex_types: input
            .complex_types
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
        annotations: input.annotations.map_type(|t| replace(&t, &replacements)),
        actions: map_types_in_actions(input.actions, |t| replace(&t, &replacements)),
    }
}
