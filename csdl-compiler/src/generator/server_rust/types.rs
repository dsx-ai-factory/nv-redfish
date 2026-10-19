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

use std::fmt::{self, Display, Formatter};

use proc_macro2::{Ident, TokenStream};
use quote::{quote, ToTokens};

use super::Error;
use crate::compiler::QualifiedName;
use crate::generator::common_rust::type_name::TypeNameForExcerptCopy;
use crate::generator::common_rust::{ident::escaped, ModName, TypeName};
use crate::redfish::ExcerptCopy;

/// A full schema model or a selected excerpt of that model.
#[derive(Clone, Copy)]
pub(super) enum ResponseTypeName<'a> {
    Schema(TypeName<'a>),
    Excerpt(TypeNameForExcerptCopy<'a>),
}

impl Display for ResponseTypeName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Schema(name) => Display::fmt(name, f),
            Self::Excerpt(name) => Display::fmt(name, f),
        }
    }
}

impl ToTokens for ResponseTypeName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match self {
            Self::Schema(name) => name.to_tokens(tokens),
            Self::Excerpt(name) => name.to_tokens(tokens),
        }
    }
}

/// Name of the required-fields helper associated with a schema type.
#[derive(Clone, Copy)]
pub(super) struct RequiredTypeName<'a>(pub ResponseTypeName<'a>);

impl RequiredTypeName<'_> {
    pub(super) fn ident(self) -> Ident {
        escaped(&format!("{}Required", self.0))
    }
}

impl ToTokens for RequiredTypeName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.ident().to_tokens(tokens);
    }
}

pub(super) const fn type_name(name: QualifiedName<'_>) -> TypeName<'_> {
    TypeName::new_qualified(name.name)
}

pub(super) fn namespace(name: QualifiedName<'_>) -> Vec<ModName<'_>> {
    (0..name.namespace.len())
        .filter_map(|depth| name.namespace.get_id(depth))
        .map(ModName::new)
        .collect()
}

pub(super) fn path(name: QualifiedName<'_>, depth: usize) -> TokenStream {
    path_with_name(name, depth, type_name(name))
}

pub(super) fn excerpt_path(
    name: QualifiedName<'_>,
    excerpt: &ExcerptCopy,
    depth: usize,
) -> TokenStream {
    path_with_name(name, depth, type_name(name).for_excerpt_copy(excerpt))
}

fn path_with_name(name: QualifiedName<'_>, depth: usize, type_name: impl ToTokens) -> TokenStream {
    let parents = (0..depth).map(|_| quote! { super:: });
    let modules = namespace(name);
    quote! { #(#parents)* #(#modules::)* #type_name }
}

pub(super) fn is_string(name: QualifiedName<'_>) -> bool {
    name.namespace.is_edm()
        && matches!(
            name.name.inner().as_str(),
            "String" | "DateTimeOffset" | "Duration" | "Date" | "TimeOfDay" | "Binary"
        )
}

pub(super) fn primitive(name: QualifiedName<'_>) -> Result<TokenStream, Error> {
    if is_string(name) {
        return Ok(quote! { std::string::String });
    }
    match name.name.inner().as_str() {
        "Guid" => Ok(quote! { nv_redfish_server_core::EdmGuid }),
        "Boolean" => Ok(quote! { bool }),
        "SByte" => Ok(quote! { i8 }),
        "Byte" => Ok(quote! { u8 }),
        "Int16" => Ok(quote! { i16 }),
        "Int32" => Ok(quote! { i32 }),
        "Int64" => Ok(quote! { i64 }),
        "Double" | "Single" | "Decimal" => Ok(quote! { nv_redfish_server_core::FiniteF64 }),
        _ => Err(Error::UnsupportedPrimitive(name.into())),
    }
}
