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

use crate::compiler::TypeClass;
use crate::edmx::attribute_values::SimpleIdentifier;
use crate::edmx::ActionName as EdmxActionName;
use crate::edmx::ParameterName;
use crate::generator::casemungler;
use crate::generator::common_rust::ident;
use crate::redfish::ExcerptCopy;
use proc_macro2::TokenStream;
use quote::ToTokens;
use quote::TokenStreamExt as _;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub enum TypeName<'a> {
    Qualified(&'a SimpleIdentifier),
    Action {
        binding_name: &'a ParameterName,
        action_name: &'a EdmxActionName,
    },
}

impl<'a> TypeName<'a> {
    #[must_use]
    pub const fn new_qualified(v: &'a SimpleIdentifier) -> Self {
        Self::Qualified(v)
    }

    #[must_use]
    pub const fn new_action(
        binding_name: &'a ParameterName,
        action_name: &'a EdmxActionName,
    ) -> Self {
        Self::Action {
            binding_name,
            action_name,
        }
    }

    #[must_use]
    pub const fn for_update(&self, type_class: Option<TypeClass>) -> TypeNameForUpdate<'a> {
        TypeNameForUpdate(*self, type_class)
    }

    #[must_use]
    pub const fn for_create(&self) -> TypeNameForCreate<'a> {
        TypeNameForCreate(*self)
    }

    #[must_use]
    pub const fn for_excerpt_copy(&self, excerpt: &'a ExcerptCopy) -> TypeNameForExcerptCopy<'a> {
        TypeNameForExcerptCopy(*self, excerpt)
    }
}

impl ToTokens for TypeName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(ident::escaped(&self.to_string()));
    }
}

impl Display for TypeName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Qualified(v) => f.write_str(&casemungler::to_camel(v.inner())),

            Self::Action {
                binding_name,
                action_name,
            } => {
                write!(f, "{binding_name}{action_name}Action")
            }
        }
    }
}

impl Debug for TypeName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(self, f)
    }
}

pub struct TypeNameForUpdate<'a>(TypeName<'a>, Option<TypeClass>);

impl Display for TypeNameForUpdate<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        if self.1.is_none_or(|v| v == TypeClass::ComplexType) {
            write!(f, "{}Update", self.0)
        } else {
            Display::fmt(&self.0, f)
        }
    }
}

impl ToTokens for TypeNameForUpdate<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(ident::escaped(&self.to_string()));
    }
}

pub struct TypeNameForCreate<'a>(TypeName<'a>);

impl Display for TypeNameForCreate<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}Create", self.0)
    }
}

impl ToTokens for TypeNameForCreate<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(ident::escaped(&self.to_string()));
    }
}

#[derive(Clone, Copy)]
pub struct TypeNameForExcerptCopy<'a>(TypeName<'a>, &'a ExcerptCopy);

impl Display for TypeNameForExcerptCopy<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self.1 {
            ExcerptCopy::AllKeys => write!(f, "{}Excerpt", self.0),
            ExcerptCopy::Key(key) => write!(f, "{}Excerpt{key}", self.0),
        }
    }
}

impl ToTokens for TypeNameForExcerptCopy<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(ident::escaped(&self.to_string()));
    }
}
