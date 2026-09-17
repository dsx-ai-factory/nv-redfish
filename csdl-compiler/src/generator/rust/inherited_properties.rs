// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Borrow schema fields through inheritance for every generated model shape.

use crate::compiler::odata::MustHaveType;
use crate::compiler::{Action, Compiled, Properties, QualifiedName};
use crate::odata::annotations::AdditionalProperties;
use crate::redfish::DynamicProperties;

/// Properties and inherited metadata, ordered from ancestors to the concrete type.
#[derive(Debug)]
pub struct InheritedProperties<'a> {
    pub properties: Vec<&'a Properties<'a>>,
    pub actions: Vec<&'a Action<'a>>,
    pub must_have_type: MustHaveType,
    pub additional_properties: Option<AdditionalProperties>,
    pub dynamic_properties: Option<DynamicProperties<'a>>,
}

impl<'a> InheritedProperties<'a> {
    #[must_use]
    pub fn new(compiled: &'a Compiled<'a>, name: QualifiedName<'a>) -> Self {
        let mut result = Self {
            properties: Vec::new(),
            actions: Vec::new(),
            must_have_type: MustHaveType::new(false),
            additional_properties: None,
            dynamic_properties: None,
        };
        let mut next = Some(name);
        while let Some(name) = next {
            let (base, properties, odata, dynamic_properties) =
                if let Some(t) = compiled.complex_types.get(&name) {
                    (t.base, &t.properties, t.odata, t.redfish.dynamic_properties)
                } else if let Some(t) = compiled.entity_types.get(&name) {
                    (t.base, &t.properties, t.odata, None)
                } else {
                    break;
                };
            result.properties.push(properties);
            result.actions.extend(
                compiled
                    .actions
                    .get(&name)
                    .into_iter()
                    .flat_map(|a| a.values()),
            );
            if odata.must_have_type.into_inner() {
                result.must_have_type = odata.must_have_type;
            }
            result.additional_properties =
                result.additional_properties.or(odata.additional_properties);
            result.dynamic_properties = result.dynamic_properties.or(dynamic_properties);
            next = base;
        }
        result.properties.reverse();
        result.actions.sort_by_key(|a| a.name);
        result
    }
}
