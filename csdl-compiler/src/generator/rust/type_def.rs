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

use crate::compiler::TypeDefinition;
use crate::generator::rust::Config;
use crate::generator::rust::FullTypeName;
use crate::generator::rust::TypeName;
use proc_macro2::TokenStream;
use quote::quote;

/// Type definition that maps to simple type.
#[derive(Debug)]
pub struct TypeDef<'a> {
    pub name: TypeName<'a>,
    pub compiled: &'a TypeDefinition<'a>,
}

impl TypeDef<'_> {
    /// Generate rust code for types derived from simples type (type
    /// definitions and enums).
    pub fn generate(self, tokens: &mut TokenStream, config: &Config) {
        let name = self.name;
        let underlying_type = FullTypeName::new(self.compiled.underlying_type, config);
        tokens.extend(quote! {
            pub type #name = #underlying_type;
        });
    }
}
