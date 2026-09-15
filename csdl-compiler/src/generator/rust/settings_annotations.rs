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

//! Generate the container for settings annotations on resources.

use crate::compiler::annotations::Annotations;
use crate::generator::rust::{doc, Config, FullTypeName};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

/// Generate settings annotations using the resolved and optimized schema types.
#[must_use]
pub fn generate(annotations: &Annotations<'_>, config: &Config) -> TokenStream {
    let serialize = config
        .serialize_read_models
        .then(|| quote! { #[derive(serde::Serialize)] });
    let config = Config {
        top_module_alias: Ident::new("self", Span::call_site()),
        ..Config::default()
    };
    let settings = annotations.redfish_settings.map(|annotation| {
        let value_type = FullTypeName::new(annotation.value_type, &config);
        let field_doc = doc::format_and_generate(&annotation.term.name, &annotation.odata());
        let wire_name = format!("@Redfish.{}", annotation.term.name);
        quote! {
            #field_doc
            #[serde(rename = #wire_name, skip_serializing_if = "Option::is_none")]
            pub settings: Option<#value_type>,
        }
    });
    let settings_apply_time = annotations.redfish_settings_apply_time.map(|annotation| {
        let value_type = FullTypeName::new(annotation.value_type, &config);
        let field_doc = doc::format_and_generate(&annotation.term.name, &annotation.odata());
        let wire_name = format!("@Redfish.{}", annotation.term.name);
        quote! {
            #field_doc
            #[serde(rename = #wire_name, skip_serializing_if = "Option::is_none")]
            pub settings_apply_time: Option<#value_type>,
        }
    });
    quote! {
        /// Optional settings annotations flattened into a resource object.
        #serialize
        #[derive(serde::Deserialize, Debug, Default)]
        #[non_exhaustive]
        pub struct SettingsAnnotations {
            #settings
            #settings_apply_time
        }
    }
}
