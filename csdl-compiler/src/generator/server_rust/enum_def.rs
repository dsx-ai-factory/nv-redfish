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

use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::quote;

use super::{types, Error};
use crate::compiler::EnumType;
use crate::generator::casemungler::to_camel;
use crate::generator::common_rust::{doc::format_and_generate, ident::escaped, TypeName};

pub(super) struct EnumDef<'a> {
    pub(super) name: TypeName<'a>,
    compiled: &'a EnumType<'a>,
}

impl<'a> EnumDef<'a> {
    pub(super) const fn new(compiled: &'a EnumType<'a>) -> Self {
        Self {
            name: types::type_name(compiled.name),
            compiled,
        }
    }

    pub(super) fn generate(self) -> Result<TokenStream, Error> {
        let name = self.name;
        let value = self.compiled;
        let docs = format_and_generate(name, &value.odata);
        let (variants, arms, _) = value.members.iter().try_fold(
            (Vec::new(), Vec::new(), BTreeSet::new()),
            |(mut variants, mut arms, mut names), member| {
                let wire = member.name.inner().inner();
                let variant = escaped(&to_camel(wire));
                if names.insert(variant.to_string()) {
                    let docs = format_and_generate(member.name, &member.odata);
                    variants.push(quote! { #docs #[serde(rename = #wire)] #variant, });
                    arms.push(quote! { Self::#variant => #wire, });
                    Ok((variants, arms, names))
                } else {
                    let path = types::path(value.name, 0);
                    Err(Error::NameCollision(syn::parse_quote!(#path::#variant)))
                }
            },
        )?;
        Ok(quote! {
            #docs
            #[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
            pub enum #name { #(#variants)* }
            impl nv_redfish_server_core::Enum for #name {
                fn as_str(self) -> &'static str {
                    match self { #(#arms)* }
                }
            }
        })
    }
}
