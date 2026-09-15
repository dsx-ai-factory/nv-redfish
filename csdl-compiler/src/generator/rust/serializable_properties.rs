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

use crate::compiler::NavProperty;
use crate::compiler::Properties;
use crate::compiler::Property;
use crate::compiler::TypeClass;
use crate::generator::rust::Config;
use crate::generator::rust::FullTypeName;
use crate::generator::rust::StructFieldName;
use crate::odata::annotations::Permissions;
use crate::OneOrCollection;
use proc_macro2::Ident;
use proc_macro2::Literal;
use proc_macro2::Span;
use proc_macro2::TokenStream;
use quote::quote;
use quote::ToTokens;

/// A compiled property that can be emitted in a create or update request structure.
struct SerializableProperty<'a> {
    /// The Redfish property name used by serde on the wire.
    rename: Literal,
    /// The generated Rust field name.
    name: StructFieldName<'a>,
    /// The generated Rust type, excluding any request-specific optional wrapper.
    prop_type: TokenStream,
    /// Whether the property is a required argument when constructing a create request.
    required_on_create: bool,
    /// Whether the property may be written but not read.
    write_only: bool,
}

/// Properties selected for serialization in generated create and update request structures.
pub struct SerializableProperties<'a>(Vec<SerializableProperty<'a>>);

trait IntoTokenStream {
    fn into_token_stream(self) -> TokenStream;
}

impl<I: Iterator<Item = TokenStream>> IntoTokenStream for I {
    fn into_token_stream(self) -> TokenStream {
        let mut stream = TokenStream::new();
        stream.extend(self);
        stream
    }
}

impl<'a> SerializableProperties<'a> {
    /// Selects properties that are required on create or writable
    /// according to their Redfish and `OData` annotations, and
    /// computes their generated Rust names and types.
    #[must_use]
    pub fn for_update(properties: &Properties<'a>, config: &Config) -> Self {
        Self(
            properties
                .properties
                .iter()
                .filter_map(|p| {
                    let (typeinfo, v) = &p.ptype.inner();
                    if !(p.redfish.is_required_on_create.into_inner()
                        || (p.odata.permissions_is_write()
                            && typeinfo.permissions.is_none_or(|p| p != Permissions::Read)))
                    {
                        return None;
                    }

                    let full_type = FullTypeName::new(*v, config).for_update(Some(typeinfo.class));
                    Some(Self::structural_property(p, full_type))
                })
                .collect(),
        )
    }

    /// Selects all properties, including inherited and read-only properties, for creation.
    /// Nested complex values use their own create shapes; navigation values are references.
    #[must_use]
    pub fn for_create(properties: &[&Properties<'a>], config: &Config) -> Self {
        let top = &config.top_module_alias;
        Self(
            properties
                .iter()
                .flat_map(|properties| {
                    let structural = properties.properties.iter().map(|p| {
                        let (typeinfo, name) = p.ptype.inner();
                        let full_type = FullTypeName::new(*name, config);
                        let full_type = if typeinfo.class == TypeClass::ComplexType {
                            full_type.for_create().to_token_stream()
                        } else {
                            full_type.to_token_stream()
                        };
                        Self::structural_property(p, full_type)
                    });
                    let navigation = properties
                        .nav_properties
                        .iter()
                        .map(|p| Self::navigation_property(p, quote! { #top::Reference }));
                    structural.chain(navigation)
                })
                .collect(),
        )
    }

    fn structural_property(p: &Property<'a>, full_type: impl ToTokens) -> SerializableProperty<'a> {
        let prop_type = match p.ptype {
            OneOrCollection::One(_) => quote! { #full_type },
            OneOrCollection::Collection(_) => {
                if p.rigid_array_support.into_inner() {
                    quote! { Vec<Option<#full_type>> }
                } else {
                    quote! { Vec<#full_type> }
                }
            }
        };
        SerializableProperty {
            rename: Literal::string(p.name.inner().inner()),
            name: StructFieldName::new_property(p.name),
            prop_type,
            required_on_create: p.redfish.is_required_on_create.into_inner(),
            write_only: p.odata.permissions_is_write_only(),
        }
    }

    fn navigation_property(
        p: &NavProperty<'a>,
        full_type: impl ToTokens,
    ) -> SerializableProperty<'a> {
        let (cardinality, name, odata, redfish) = match p {
            NavProperty::Expandable(p) => {
                (p.ptype.as_ref().map(|_| ()), p.name, &p.odata, &p.redfish)
            }
            NavProperty::Reference {
                cardinality,
                odata,
                redfish,
            } => (
                cardinality.as_ref().map(|_| ()),
                *cardinality.inner(),
                odata,
                redfish,
            ),
        };
        SerializableProperty {
            rename: Literal::string(name.inner().inner()),
            name: StructFieldName::new_property(name),
            prop_type: match cardinality {
                OneOrCollection::One(()) => quote! { #full_type },
                OneOrCollection::Collection(()) => quote! { Vec<#full_type> },
            },
            required_on_create: redfish.is_required_on_create.into_inner(),
            write_only: odata.permissions_is_write_only(),
        }
    }

    /// Generates the field declarations for an update request structure.
    ///
    /// Every field is optional and omitted from the serialized request when it is not set.
    #[must_use]
    pub fn struct_content_for_update(&self) -> TokenStream {
        self.0
            .iter()
            .map(|p| {
                let rename = &p.rename;
                let name = p.name;
                let prop_type = &p.prop_type;
                quote! {
                    #[serde(rename=#rename)]
                    #[serde(skip_serializing_if = "Option::is_none")]
                    pub #name: Option<#prop_type>,
                }
            })
            .into_token_stream()
    }

    /// Generates the field declarations for a create request structure.
    ///
    /// Properties required on create are emitted directly; all other fields are optional and
    /// omitted from the serialized request when they are not set.
    #[must_use]
    pub fn struct_content_for_create(&self) -> TokenStream {
        self.0
            .iter()
            .map(|p| {
                let rename = &p.rename;
                let name = p.name;
                let prop_type = &p.prop_type;
                if p.required_on_create {
                    quote! {
                        #[serde(rename=#rename)]
                        pub #name: #prop_type,
                    }
                } else {
                    quote! {
                        #[serde(rename=#rename)]
                        #[serde(skip_serializing_if = "Option::is_none")]
                        pub #name: Option<#prop_type>,
                    }
                }
            })
            .into_token_stream()
    }

    /// Generates builder setters for create-request properties that are not required on create.
    #[must_use]
    pub fn optional_property_setter_for_create(&self) -> TokenStream {
        self.0
            .iter()
            .filter_map(|p| {
                if p.required_on_create {
                    None
                } else {
                    Some(Self::generate_optional_property_setter(p))
                }
            })
            .into_token_stream()
    }

    /// Generates builder setters for every update-request property.
    #[must_use]
    pub fn optional_property_setter_for_update(&self) -> TokenStream {
        self.0
            .iter()
            .map(Self::generate_optional_property_setter)
            .into_token_stream()
    }

    /// Returns whether any selected property is write-only and therefore potentially sensitive.
    #[must_use]
    pub fn can_contain_sensitive_info(&self) -> bool {
        self.0.iter().any(|p| p.write_only)
    }

    /// Generates `DebugStruct::field` calls for a create request.
    ///
    /// Write-only values are replaced with a redaction marker. For optional write-only fields,
    /// the generated output preserves whether the field was set without exposing its value.
    #[must_use]
    pub fn debug_print_fields_for_create(&self) -> TokenStream {
        self.0
            .iter()
            .map(|p| {
                let name = p.name;
                let debug_name = Literal::string(&name.to_string());
                if p.write_only {
                    if p.required_on_create {
                        quote! { .field(#debug_name, &"<redacted>") }
                    } else {
                        quote! { .field(#debug_name, &self.#name.as_ref().map(|_| "<redacted>")) }
                    }
                } else {
                    quote! { .field(#debug_name, &self.#name) }
                }
            })
            .into_token_stream()
    }

    /// Generates `DebugStruct::field` calls for an update request.
    ///
    /// Write-only values are replaced with a redaction marker while preserving whether each
    /// optional field was set.
    #[must_use]
    pub fn debug_print_fields_for_update(&self) -> TokenStream {
        self.0
            .iter()
            .map(|p| {
                let name = p.name;
                let debug_name = Literal::string(&name.to_string());
                if p.write_only {
                    quote! { .field(#debug_name, &self.#name.as_ref().map(|_| "<redacted>")) }
                } else {
                    quote! { .field(#debug_name, &self.#name) }
                }
            })
            .into_token_stream()
    }

    /// Generates the required-property arguments for a create request's `builder` function.
    #[must_use]
    pub fn builder_fn_arg_list_for_create(&self) -> TokenStream {
        self.0
            .iter()
            .filter_map(|p| {
                let name = p.name;
                let prop_type = &p.prop_type;
                if p.required_on_create {
                    Some(quote! {#name: #prop_type,})
                } else {
                    None
                }
            })
            .into_token_stream()
    }

    /// Generates the field initializers for a create request's `builder` function.
    ///
    /// Required properties are initialized from arguments and optional properties are initialized
    /// to `None`.
    #[must_use]
    pub fn builder_fn_content_for_create(&self) -> TokenStream {
        self.0
            .iter()
            .map(|p| {
                let name = p.name;
                if p.required_on_create {
                    quote! { #name, }
                } else {
                    quote! { #name: None, }
                }
            })
            .into_token_stream()
    }

    fn generate_optional_property_setter(p: &SerializableProperty<'a>) -> TokenStream {
        let name = p.name;
        let prop_type = &p.prop_type;
        // Field names for digit-leading properties carry a leading underscore.
        let fn_name = Ident::new(
            &format!("with_{}", name.to_string().trim_start_matches('_')),
            Span::call_site(),
        );
        quote! {
            #[must_use]
            pub fn #fn_name(mut self, v: #prop_type) -> Self {
                self.#name = Some(v);
                self
            }
        }
    }
}
