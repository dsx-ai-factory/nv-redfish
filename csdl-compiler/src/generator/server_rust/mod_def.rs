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

use std::collections::{BTreeMap, BTreeSet};
use std::iter::once;

use proc_macro2::{Ident, TokenStream};
use quote::quote;

use super::{
    enum_def::EnumDef,
    struct_def::StructDef,
    type_def::TypeDef,
    types::{RequiredTypeName, ResponseTypeName},
    Error,
};
use crate::compiler::QualifiedName;
use crate::generator::common_rust::{ident::escaped, ModName};

pub(super) enum Definition<'a> {
    Struct(StructDef<'a>),
    Enum(EnumDef<'a>),
    Type(TypeDef<'a>),
}

impl<'a> Definition<'a> {
    const fn name(&self) -> ResponseTypeName<'a> {
        match self {
            Self::Struct(definition) => definition.name,
            Self::Enum(definition) => ResponseTypeName::Schema(definition.name),
            Self::Type(definition) => ResponseTypeName::Schema(definition.name),
        }
    }

    const fn helper(&self) -> Option<RequiredTypeName<'a>> {
        match self {
            Self::Struct(definition) => definition.helper,
            _ => None,
        }
    }

    fn generate(self) -> Result<TokenStream, Error> {
        match self {
            Self::Struct(definition) => definition.generate(),
            Self::Enum(definition) => definition.generate(),
            Self::Type(definition) => definition.generate(),
        }
    }
}

#[derive(Default)]
pub(super) struct ModDef<'a> {
    definitions: Vec<Definition<'a>>,
    sub_mods: BTreeMap<ModName<'a>, Self>,
}

impl<'a> ModDef<'a> {
    pub(super) fn add(&mut self, name: QualifiedName<'a>, definition: Definition<'a>) {
        let mut module = self;
        for segment in super::types::namespace(name) {
            module = module.sub_mods.entry(segment).or_default();
        }
        module.definitions.push(definition);
    }

    fn merge(&mut self, other: Self) {
        self.definitions.extend(other.definitions);
        for (name, child) in other.sub_mods {
            self.sub_mods.entry(name).or_default().merge(child);
        }
    }

    pub(super) fn generate(self) -> Result<TokenStream, Error> {
        let mut names = BTreeSet::new();
        let mut content = TokenStream::new();
        for definition in self.definitions {
            for identifier in once(escaped(&definition.name().to_string()))
                .chain(definition.helper().map(RequiredTypeName::ident))
            {
                if !names.insert(identifier.clone()) {
                    return Err(Error::NameCollision(identifier.into()));
                }
            }
            content.extend(definition.generate()?);
        }

        // Merge source namespaces that produce the same Rust module name before
        // emitting their contents, so collision checks see all declarations.
        let mut sub_mods: BTreeMap<Ident, Self> = BTreeMap::new();
        for (name, module) in self.sub_mods {
            sub_mods
                .entry(escaped(&name.to_string()))
                .or_default()
                .merge(module);
        }
        for (name, module) in sub_mods {
            let body = module.generate()?;
            content.extend(quote! { pub mod #name { #body } });
        }
        Ok(content)
    }
}
