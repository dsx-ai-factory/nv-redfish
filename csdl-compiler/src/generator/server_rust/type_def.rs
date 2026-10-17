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

use proc_macro2::TokenStream;
use quote::quote;

use super::{types, Error};
use crate::compiler::TypeDefinition;
use crate::generator::common_rust::TypeName;

pub(super) struct TypeDef<'a> {
    pub(super) name: TypeName<'a>,
    compiled: &'a TypeDefinition<'a>,
}

impl<'a> TypeDef<'a> {
    pub(super) const fn new(compiled: &'a TypeDefinition<'a>) -> Self {
        Self {
            name: types::type_name(compiled.name),
            compiled,
        }
    }

    pub(super) fn generate(self) -> Result<TokenStream, Error> {
        let name = self.name;
        let underlying_type = types::primitive(self.compiled.underlying_type)?;
        Ok(quote! { pub type #name = #underlying_type; })
    }
}
