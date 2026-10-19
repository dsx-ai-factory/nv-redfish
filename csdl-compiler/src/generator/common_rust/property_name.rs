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

use crate::edmx::ParameterName as EdmxParameterName;
use crate::edmx::PropertyName as EdmxPropertyName;
use crate::generator::casemungler;
use crate::generator::common_rust::ident;
use proc_macro2::{Literal, TokenStream};
use quote::ToTokens;
use quote::TokenStreamExt as _;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;

/// Property name built from edmx `PropertyName`.
///
/// Example of representation: `protocol_features_supported`
#[derive(PartialEq, Eq, Hash, Copy, Clone, Ord, PartialOrd)]
pub enum StructFieldName<'a> {
    Property(&'a EdmxPropertyName),
    Parameter(&'a EdmxParameterName),
}

impl<'a> StructFieldName<'a> {
    /// Create new by property name.
    #[must_use]
    pub const fn new_property(v: &'a EdmxPropertyName) -> Self {
        Self::Property(v)
    }
    /// Create new by parameter name.
    #[must_use]
    pub const fn new_parameter(v: &'a EdmxParameterName) -> Self {
        Self::Parameter(v)
    }

    /// Emit the original CSDL name as a serde string literal.
    #[must_use]
    pub fn rename(self) -> Literal {
        match self {
            Self::Property(name) => Literal::string(name.inner().inner()),
            Self::Parameter(name) => Literal::string(name.inner().inner()),
        }
    }
}

impl ToTokens for StructFieldName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        tokens.append(ident::escaped(&self.to_string()));
    }
}

impl Display for StructFieldName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Property(v) => f.write_str(&casemungler::to_snake(v.inner())),
            Self::Parameter(v) => f.write_str(&casemungler::to_snake(v.inner())),
        }
    }
}

impl Debug for StructFieldName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(self, f)
    }
}
