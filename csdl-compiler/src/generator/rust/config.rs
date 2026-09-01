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

use crate::edmx::PropertyName;
use proc_macro2::Ident;
use proc_macro2::Span;

/// Configuration of Generation
pub struct Config {
    /// Top module alias that is defined in each submodule.
    pub top_module_alias: Ident,
    /// When one type derived from another we add `#serde(flatten)`
    /// property to generated code. The name for the property is
    /// defined by this parameter.
    pub base_type_prop_name: PropertyName,

    /// Maximum number of parameters that are passed as function
    /// parameter before switching to action struct.
    pub action_fn_max_param_number_threshold: usize,

    /// Whether generated read and excerpt models implement `serde::Serialize`.
    pub serialize_read_models: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            top_module_alias: Ident::new("redfish", Span::call_site()),
            base_type_prop_name: PropertyName::new(
                "Base".parse().expect("should always be parsed"),
            ),
            action_fn_max_param_number_threshold: 3,
            serialize_read_models: false,
        }
    }
}
