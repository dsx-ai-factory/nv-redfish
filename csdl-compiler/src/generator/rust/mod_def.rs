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

use crate::compiler::Action;
use crate::compiler::ActionsMap;
use crate::compiler::ComplexType;
use crate::compiler::EntityType;
use crate::compiler::EnumType;
use crate::compiler::ForcedUpdate;
use crate::compiler::IsCreatable;
use crate::compiler::Namespace;
use crate::compiler::TypeDefinition;
use crate::generator::rust::doc;
use crate::generator::rust::struct_def::GenerateType;
use crate::generator::rust::Config;
use crate::generator::rust::EnumDef;
use crate::generator::rust::Error;
use crate::generator::rust::ModName;
use crate::generator::rust::StructDef;
use crate::generator::rust::TypeDef;
use crate::generator::rust::TypeName;
use crate::redfish::ExcerptCopy;
use proc_macro2::Ident;
use proc_macro2::Punct;
use proc_macro2::Spacing;
use proc_macro2::Span;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use quote::quote;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::iter::repeat_n;

#[derive(Debug, Default)]
pub struct ModDef<'a> {
    name: Option<ModName<'a>>,
    namespace: Option<Namespace<'a>>,
    typedefs: HashMap<TypeName<'a>, TypeDef<'a>>,
    enums: HashMap<TypeName<'a>, EnumDef<'a>>,
    structs: HashMap<TypeName<'a>, StructDef<'a>>,
    sub_mods: HashMap<ModName<'a>, Self>,
    depth: usize,
}

impl<'a> ModDef<'a> {
    #[must_use]
    pub fn new(name: ModName<'a>, namespace: Namespace<'a>, depth: usize) -> Self {
        Self {
            name: Some(name),
            // A module is shared by every schema version that maps to
            // it, so record only the namespace prefix that names this
            // module; the full namespace of whichever type happened to
            // create it would otherwise leak into the doc comment and
            // vary with map iteration order.
            namespace: Some(namespace.truncated(depth + 1)),
            structs: HashMap::new(),
            sub_mods: HashMap::new(),
            enums: HashMap::new(),
            typedefs: HashMap::new(),
            depth,
        }
    }

    /// Add complex type to the module.
    ///
    /// # Errors
    ///
    /// Returns `CreateStruct` error if failed to add new struct to the
    /// module.  it may only happen in case of name conflicts because
    /// of case conversion.
    pub fn add_complex_type(
        self,
        ct: ComplexType<'a>,
        actions: ActionsMap<'a>,
        forced_update: ForcedUpdate,
        config: &Config,
    ) -> Result<Self, Error<'a>> {
        self.inner_add_complex_type(ct, 0, actions, forced_update, config)
    }

    fn inner_add_complex_type(
        mut self,
        ct: ComplexType<'a>,
        depth: usize,
        actions: ActionsMap<'a>,
        forced_update: ForcedUpdate,
        config: &Config,
    ) -> Result<Self, Error<'a>> {
        if let Some(id) = ct.name.namespace.get_id(depth) {
            let mod_name = ModName::new(id);
            self.sub_mods
                .remove(&mod_name)
                .unwrap_or_else(|| ModDef::new(mod_name, ct.name.namespace, depth))
                .inner_add_complex_type(ct, depth + 1, actions, forced_update, config)
                .map(|submod| {
                    self.sub_mods.insert(mod_name, submod);
                    self
                })
        } else {
            let struct_name = TypeName::new_qualified(ct.name.name);
            let builder = StructDef::builder(struct_name, ct.odata);
            let builder = if let Some(base) = ct.base {
                builder.with_base(base)
            } else {
                builder
            };
            let builder = if let Some(dynamic_properties) = ct.redfish.dynamic_properties {
                builder.with_dynamic_properties(dynamic_properties)
            } else {
                builder
            };
            // If complex type cannot be used for updates then skip
            // generation of Update structures.
            let builder = if ct.generates_update() || forced_update.into_inner() {
                builder.with_generate_type(vec![GenerateType::Read, GenerateType::Update])
            } else {
                builder.with_generate_type(vec![GenerateType::Read])
            };
            let struct_def = builder
                .with_properties(ct.properties)
                .with_actions(actions)
                .build(config)?;
            self.add_struct_def(struct_def)
                .map_err(Box::new)
                .map_err(|e| Error::CreateStruct(struct_name, e))
        }
    }

    /// Add enum type to the module.
    ///
    /// # Errors
    ///
    /// TODO
    pub fn add_enum_type(self, t: EnumType<'a>) -> Result<Self, Error<'a>> {
        self.inner_add_enum_type(t, 0)
    }

    fn inner_add_enum_type(mut self, t: EnumType<'a>, depth: usize) -> Result<Self, Error<'a>> {
        if let Some(id) = t.name.namespace.get_id(depth) {
            let mod_name = ModName::new(id);
            self.sub_mods
                .remove(&mod_name)
                .unwrap_or_else(|| ModDef::new(mod_name, t.name.namespace, depth))
                .inner_add_enum_type(t, depth + 1)
                .map(|submod| {
                    self.sub_mods.insert(mod_name, submod);
                    self
                })
        } else {
            let name = t.name;
            let type_name = TypeName::new_qualified(t.name.name);
            match self.enums.entry(type_name) {
                Entry::Occupied(_) => Err(Error::NameConflict),
                Entry::Vacant(v) => {
                    v.insert(EnumDef {
                        name: type_name,
                        compiled: t,
                    });
                    Ok(self)
                }
            }
            .map_err(Box::new)
            .map_err(|e| Error::CreateSimplType(name, e))
        }
    }

    /// Add type definition to the module.
    ///
    /// # Errors
    ///
    ///
    pub fn add_type_definition(self, t: TypeDefinition<'a>) -> Result<Self, Error<'a>> {
        self.inner_add_type_definition(t, 0)
    }

    fn inner_add_type_definition(
        mut self,
        t: TypeDefinition<'a>,
        depth: usize,
    ) -> Result<Self, Error<'a>> {
        if let Some(id) = t.name.namespace.get_id(depth) {
            let mod_name = ModName::new(id);
            self.sub_mods
                .remove(&mod_name)
                .unwrap_or_else(|| ModDef::new(mod_name, t.name.namespace, depth))
                .inner_add_type_definition(t, depth + 1)
                .map(|submod| {
                    self.sub_mods.insert(mod_name, submod);
                    self
                })
        } else {
            let name = t.name;
            let type_name = TypeName::new_qualified(t.name.name);
            match self.typedefs.entry(type_name) {
                Entry::Occupied(_) => Err(Error::NameConflict),
                Entry::Vacant(v) => {
                    v.insert(TypeDef {
                        name: type_name,
                        compiled: t,
                    });
                    Ok(self)
                }
            }
            .map_err(Box::new)
            .map_err(|e| Error::CreateSimplType(name, e))
        }
    }

    /// Add entity type to the module.
    ///
    /// # Errors
    ///
    /// Returns `CreateStruct` error if failed to add new struct to the
    /// module.  it may only happen in case of name conflicts because
    /// of case conversion.
    pub fn add_entity_type(
        self,
        t: EntityType<'a>,
        creatable: IsCreatable,
        excerpt_copies: Vec<ExcerptCopy>,
        forced_update: ForcedUpdate,
        config: &Config,
    ) -> Result<Self, Error<'a>> {
        self.inner_add_entity_type(t, creatable, excerpt_copies, forced_update, 0, config)
    }

    fn inner_add_entity_type(
        mut self,
        t: EntityType<'a>,
        creatable: IsCreatable,
        excerpt_copies: Vec<ExcerptCopy>,
        forced_update: ForcedUpdate,
        depth: usize,
        config: &Config,
    ) -> Result<Self, Error<'a>> {
        if let Some(id) = t.name.namespace.get_id(depth) {
            let mod_name = ModName::new(id);
            self.sub_mods
                .remove(&mod_name)
                .unwrap_or_else(|| ModDef::new(mod_name, t.name.namespace, depth))
                .inner_add_entity_type(
                    t,
                    creatable,
                    excerpt_copies,
                    forced_update,
                    depth + 1,
                    config,
                )
                .map(|submod| {
                    self.sub_mods.insert(mod_name, submod);
                    self
                })
        } else {
            let struct_name = TypeName::new_qualified(t.name.name);
            let builder = StructDef::builder(struct_name, t.odata);
            let builder = if let Some(base) = t.base {
                builder.with_base(base)
            } else {
                builder
            };
            let mut gen_types = vec![GenerateType::Read];
            let need_redfish_settings = t.generates_update();
            if need_redfish_settings || forced_update.into_inner() {
                gen_types.push(GenerateType::Update);
            }
            if creatable.into_inner() {
                gen_types.push(GenerateType::Create);
            }
            for excerpt_copy in excerpt_copies {
                gen_types.push(GenerateType::Excerpt(excerpt_copy));
            }
            // This is collection case. Members to be create are
            // defined by Members property.
            let builder = if let Some(mt) = t.insertable_member_type() {
                builder.with_create(mt)
            } else {
                builder
            };
            let builder = if need_redfish_settings {
                builder.with_redfish_settings()
            } else {
                builder
            };
            let builder = builder
                .with_properties(t.properties)
                .with_generate_type(gen_types);
            self.add_struct_def(builder.build(config)?)
                .map_err(Box::new)
                .map_err(|e| Error::CreateStruct(struct_name, e))
        }
    }

    /// Add complex type to the module.
    ///
    /// # Errors
    ///
    /// Returns `CreateStruct` error if failed to add new struct to the
    /// module.  it may only happen in case of name conflicts because
    /// of case conversion.
    pub fn add_action_type(self, t: &Action<'a>, config: &Config) -> Result<Self, Error<'a>> {
        self.inner_add_action_type(t, 0, config)
    }

    fn inner_add_action_type(
        mut self,
        t: &Action<'a>,
        depth: usize,
        config: &Config,
    ) -> Result<Self, Error<'a>> {
        if let Some(id) = t.defining_namespace.get_id(depth) {
            let mod_name = ModName::new(id);
            self.sub_mods
                .remove(&mod_name)
                .unwrap_or_else(|| ModDef::new(mod_name, t.defining_namespace, depth))
                .inner_add_action_type(t, depth + 1, config)
                .map(|submod| {
                    self.sub_mods.insert(mod_name, submod);
                    self
                })
        } else {
            let struct_name = TypeName::new_action(t.binding_name, t.name);
            let struct_def = StructDef::builder(struct_name, t.odata)
                .with_parameters(t.parameters.clone())
                .with_generate_type(vec![GenerateType::Action])
                .build(config)?;

            self.add_struct_def(struct_def)
                .map_err(Box::new)
                .map_err(|e| Error::CreateStruct(struct_name, e))
        }
    }

    fn add_struct_def(mut self, st: StructDef<'a>) -> Result<Self, Error<'a>> {
        match self.structs.entry(st.name) {
            Entry::Occupied(_) => Err(Error::NameConflict),
            Entry::Vacant(v) => {
                v.insert(st);
                Ok(self)
            }
        }
    }

    /// Generate Rust code.
    pub fn generate(self, tokens: &mut TokenStream, config: &Config) {
        let mut typedefs = self.typedefs.into_values().collect::<Vec<_>>();
        typedefs.sort_by_key(|v| v.name);

        let mut enums = self.enums.into_values().collect::<Vec<_>>();
        enums.sort_by_key(|v| v.name);

        let mut sub_mods = self.sub_mods.into_values().collect::<Vec<_>>();
        sub_mods.sort_by_key(|v| v.name);

        let mut structs = self.structs.into_values().collect::<Vec<_>>();
        structs.sort_by_key(|v| v.name);

        let generate = |ts: &mut TokenStream| {
            for t in typedefs {
                t.generate(ts, config);
            }

            for t in enums {
                t.generate(ts, config);
            }

            for s in structs {
                s.generate(ts, config);
            }

            for m in sub_mods {
                m.generate(ts, config);
            }
        };

        if let Some(name) = self.name {
            let top = &config.top_module_alias;
            let mut content = TokenStream::new();
            content.extend([
                Self::generate_ref_to_top_module(self.depth, config),
                quote! {
                    use serde::{Serialize, Deserialize};
                    use #top::{NavProperty, ODataId, ODataETag, de_optional_nullable, de_required_nullable};
                    use #top::ActionError as _;
                },
            ]);
            generate(&mut content);
            let doc = self.namespace.map_or_else(
                || doc::generate(&["Generated schema of root namespace"]),
                |ns| doc::generate(&[format!("Generated schema of {ns} namespace")]),
            );
            tokens.extend(quote! {
                #doc
                #[allow(unused_imports)]
                pub mod #name {
                    #content
                }
            });
        } else {
            generate(tokens);
        }
    }

    fn generate_ref_to_top_module(depth: usize, config: &Config) -> TokenStream {
        let top = &config.top_module_alias;
        let supers = repeat_n(
            [
                TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                TokenTree::Punct(Punct::new(':', Spacing::Joint)),
                TokenTree::Ident(Ident::new("super", Span::call_site())),
            ],
            depth + 1,
        )
        .flatten()
        .collect::<Vec<_>>();
        let mut ts = TokenStream::new();
        ts.extend(quote! {
            use super
        });
        ts.extend(supers);
        ts.extend(quote! { ::#top  as #top ; });
        ts
    }
}
