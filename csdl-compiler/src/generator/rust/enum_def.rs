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

use crate::compiler::EnumType;
use crate::edmx::attribute_values::SimpleIdentifier;
use crate::generator::casemungler;
use crate::generator::rust::doc::format_and_generate as doc_format_and_generate;
use crate::generator::rust::ident;
use crate::generator::rust::Config;
use crate::generator::rust::TypeName;
use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::Literal;
use proc_macro2::TokenStream;
use quote::quote;
use quote::ToTokens;
use quote::TokenStreamExt as _;

/// Type definition that maps to simple type.
#[derive(Debug)]
pub struct EnumDef<'a> {
    pub name: TypeName<'a>,
    pub compiled: EnumType<'a>,
}

impl EnumDef<'_> {
    /// Generate rust code for types derived from enums.
    pub fn generate(self, tokens: &mut TokenStream, config: &Config) {
        let name = self.name;
        let top = &config.top_module_alias;
        let mut members_content = TokenStream::new();
        let mut snake_case_match_arms = TokenStream::new();

        for m in self.compiled.members {
            let rename = Literal::string(m.name.inner().inner());
            let member_name = EnumMemberName::new(m.name.inner());

            let snake_case_str = casemungler::to_snake(m.name.inner().inner());
            let snake_case_literal = Literal::string(&snake_case_str);

            members_content.extend([
                doc_format_and_generate(m.name, &m.odata),
                quote! {
                    #[serde(rename=#rename)]
                    #member_name,
                },
            ]);

            snake_case_match_arms.extend(quote! {
                Self::#member_name => #snake_case_literal,
            });
        }
        members_content.extend(quote! {
            #[doc = " Fallback value for values that are not supported by current version of Redfish schema."]
            #[doc = " The original wire spelling is not retained; serialization emits `UnsupportedValue`."]
            #[serde(other)]
            UnsupportedValue,
        });
        snake_case_match_arms.extend(quote! {
            Self::UnsupportedValue => "unsupported_value",
        });
        tokens.extend([
            doc_format_and_generate(self.name, &self.compiled.odata),
            quote! {
                #[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
                #[allow(clippy::enum_variant_names)]
                pub enum #name
            },
        ]);
        tokens.append(Group::new(Delimiter::Brace, members_content));

        tokens.extend(quote! {
            impl #top::ToSnakeCase for #name {
                fn to_snake_case(&self) -> &'static str {
                    match self {
                        #snake_case_match_arms
                    }
                }
            }
        });
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
struct EnumMemberName<'a>(&'a SimpleIdentifier);

impl<'a> EnumMemberName<'a> {
    #[must_use]
    const fn new(v: &'a SimpleIdentifier) -> Self {
        Self(v)
    }
}

impl ToTokens for EnumMemberName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(ident::escaped(&casemungler::to_camel(self.0)));
    }
}
