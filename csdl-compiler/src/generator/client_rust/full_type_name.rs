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

use crate::compiler::QualifiedName;
use crate::compiler::TypeClass;
use crate::generator::client_rust::Config;
use crate::generator::client_rust::ModName;
use crate::generator::client_rust::TypeName;
use crate::redfish::ExcerptCopy;
use proc_macro2::Punct;
use proc_macro2::Spacing;
use proc_macro2::TokenStream;
use quote::quote;
use quote::ToTokens;
use quote::TokenStreamExt as _;

/// Fully quailified type name for generation of the rust code.
///
/// Example:
///
/// `redfish::service_root::ServiceRoot`
#[derive(Clone, Copy)]
pub struct FullTypeName<'a, 'config> {
    type_name: QualifiedName<'a>,
    config: &'config Config,
}

impl<'a, 'config> FullTypeName<'a, 'config> {
    /// Create new fully qualified type name.
    #[must_use]
    pub const fn new(type_name: QualifiedName<'a>, config: &'config Config) -> Self {
        Self { type_name, config }
    }

    #[must_use]
    pub const fn for_update(
        &self,
        type_class: Option<TypeClass>,
    ) -> FullTypeNameForUpdate<'a, 'config> {
        FullTypeNameForUpdate(*self, type_class)
    }

    #[must_use]
    pub const fn for_create(&self) -> FullTypeNameForCreate<'a, 'config> {
        FullTypeNameForCreate(*self)
    }

    #[must_use]
    pub const fn for_excerpt_copy(
        &self,
        excerpt: &'a ExcerptCopy,
    ) -> FullTypeNameForExcerptCopy<'a, 'config> {
        FullTypeNameForExcerptCopy(*self, excerpt)
    }

    fn namespace_to_tokens(&self, tokens: &mut TokenStream) {
        let top = &self.config.top_module_alias;
        tokens.extend(quote! { #top });
        for depth in 0..self.type_name.namespace.len() {
            if let Some(id) = self.type_name.namespace.get_id(depth) {
                let name = ModName::new(id);
                tokens.append(Punct::new(':', Spacing::Joint));
                tokens.append(Punct::new(':', Spacing::Joint));
                tokens.extend(quote! { #name });
            }
        }
    }
}

impl ToTokens for FullTypeName<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.namespace_to_tokens(tokens);
        tokens.append(Punct::new(':', Spacing::Joint));
        tokens.append(Punct::new(':', Spacing::Joint));
        let name = TypeName::new_qualified(self.type_name.name);
        tokens.extend(quote! { #name });
    }
}

pub struct FullTypeNameForUpdate<'a, 'config>(FullTypeName<'a, 'config>, Option<TypeClass>);

impl ToTokens for FullTypeNameForUpdate<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.namespace_to_tokens(tokens);
        tokens.append(Punct::new(':', Spacing::Joint));
        tokens.append(Punct::new(':', Spacing::Joint));
        let name = TypeName::new_qualified(self.0.type_name.name).for_update(self.1);
        tokens.extend(quote! { #name });
    }
}

pub struct FullTypeNameForCreate<'a, 'config>(FullTypeName<'a, 'config>);

impl ToTokens for FullTypeNameForCreate<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.namespace_to_tokens(tokens);
        tokens.append(Punct::new(':', Spacing::Joint));
        tokens.append(Punct::new(':', Spacing::Joint));
        let name = TypeName::new_qualified(self.0.type_name.name).for_create();
        tokens.extend(quote! { #name });
    }
}

pub struct FullTypeNameForExcerptCopy<'a, 'config>(FullTypeName<'a, 'config>, &'a ExcerptCopy);

impl ToTokens for FullTypeNameForExcerptCopy<'_, '_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.namespace_to_tokens(tokens);
        tokens.append(Punct::new(':', Spacing::Joint));
        tokens.append(Punct::new(':', Spacing::Joint));
        let name = TypeName::new_qualified(self.0.type_name.name).for_excerpt_copy(self.1);
        tokens.extend(quote! { #name });
    }
}
