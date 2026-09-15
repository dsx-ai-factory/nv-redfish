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

//! Generate action annotations alongside the regular schema types.

use crate::compiler::annotations::Annotation;
use crate::generator::casemungler;
use crate::generator::rust::{doc, ident, Config, FullTypeName};
use proc_macro2::{Ident, Span, TokenStream};
use quote::quote;

/// Generate the annotation container at the root of the generated schema module.
/// Its value type has already been compiled and optimized with the schema.
#[must_use]
pub fn generate(annotation: Annotation<'_>) -> TokenStream {
    let config = Config {
        top_module_alias: Ident::new("self", Span::call_site()),
        ..Config::default()
    };
    let field_doc = doc::format_and_generate(&annotation.term.name, &annotation.odata());
    let field_name = ident::escaped(&casemungler::to_snake(annotation.term.name.inner()));
    let wire_name = format!("@Redfish.{}", annotation.term.name);
    let value_type = FullTypeName::new(annotation.value_type, &config);
    quote! {
        /// Optional protocol annotations for a POST action request.
        ///
        /// An empty container adds no properties to the request. The service
        /// advertises supported apply times through `@Redfish.OperationApplyTimeSupport`.
        #[derive(serde::Serialize, Debug, Default, Clone, Copy)]
        #[non_exhaustive]
        pub struct ActionAnnotations {
            #field_doc
            #[serde(rename = #wire_name, skip_serializing_if = "Option::is_none")]
            pub #field_name: Option<#value_type>,
        }
    }
}
