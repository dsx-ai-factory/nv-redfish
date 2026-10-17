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

//! Rust field, documentation, and setter emission from collected schema properties.

use std::borrow::Cow;
use std::fmt::{self, Display, Formatter};

use proc_macro2::{Literal, TokenStream};
use quote::{quote, ToTokens};

use super::{
    annotation::Annotation,
    property::{Property, PropertyName},
    types, Error,
};
use crate::compiler::{NavProperty, TypeClass};
use crate::generator::common_rust::{
    doc::{self, format_and_generate_with_deprecation},
    ident::escaped,
    StructFieldName,
};
use crate::{IsNullable, IsRequired};

/// Rust representation prepared only while emitting a struct.
pub(super) struct PropertyDef<'a> {
    pub name: PropertyName<'a>,
    pub prop_type: TokenStream,
    pub docs: TokenStream,
    pub required: IsRequired,
    pub nullable: IsNullable,
    pub ptype: PropertyType,
}

impl PropertyName<'_> {
    fn rename(self) -> Option<Literal> {
        match self {
            Self::Property(name) => Some(StructFieldName::new_property(name).rename()),
            Self::Annotation(annotation) => Some(annotation.rename()),
            Self::AdditionalProperties => None,
        }
    }
}

impl ToTokens for PropertyName<'_> {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        match *self {
            Self::Property(name) => StructFieldName::new_property(name).to_tokens(tokens),
            Self::Annotation(annotation) => annotation.name().to_tokens(tokens),
            Self::AdditionalProperties => escaped("additional_properties").to_tokens(tokens),
        }
    }
}

impl Display for PropertyName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.to_token_stream(), f)
    }
}

pub(super) enum PropertyType {
    Value,
    String,
    Reference,
    Collection { item: TokenStream, convert: bool },
    Additional,
}

impl<'a> PropertyDef<'a> {
    pub(super) fn new(property: Property<'a>, depth: usize) -> Result<Self, Error> {
        let (prop_type, ptype, docs) = match property {
            Property::Structural {
                property,
                type_definition,
            } => {
                let (info, target) = property.ptype.inner();
                let underlying =
                    type_definition.map_or(*target, |definition| definition.underlying_type);
                let prop_type = if info.class == TypeClass::SimpleType {
                    types::primitive(*target)?
                } else {
                    types::path(*target, depth)
                };
                let ptype = if types::is_string(underlying) {
                    PropertyType::String
                } else {
                    PropertyType::Value
                };
                let docs = format_and_generate_with_deprecation(
                    property.name,
                    &property.odata,
                    property.redfish.deprecation,
                );
                (prop_type, ptype, docs)
            }
            Property::Navigation(property) => {
                let reference = || {
                    (
                        quote! { nv_redfish_server_core::Reference },
                        PropertyType::Reference,
                    )
                };
                let (odata, redfish, (prop_type, ptype)) = match property {
                    NavProperty::Expandable(value) => (
                        &value.odata,
                        &value.redfish,
                        value
                            .redfish
                            .excerpt_copy
                            .as_ref()
                            .map_or_else(reference, |excerpt| {
                                (
                                    types::excerpt_path(*value.ptype.inner(), excerpt, depth),
                                    PropertyType::Value,
                                )
                            }),
                    ),
                    NavProperty::Reference { odata, redfish, .. } => (odata, redfish, reference()),
                };
                (
                    prop_type,
                    ptype,
                    format_and_generate_with_deprecation(
                        property.name(),
                        odata,
                        redfish.deprecation,
                    ),
                )
            }
            Property::Annotation(annotation) => {
                let (prop_type, ptype) = match annotation {
                    Annotation::ODataType => (quote! { &'static str }, PropertyType::Value),
                    Annotation::Count(_) => (quote! { u64 }, PropertyType::Value),
                    _ => (quote! { std::string::String }, PropertyType::String),
                };
                (prop_type, ptype, annotation.documentation())
            }
            Property::Additional => (
                quote! { nv_redfish_server_core::AdditionalProperties },
                PropertyType::Additional,
                doc::generate(&["Extension properties allowed by this schema."]),
            ),
        };
        let (prop_type, ptype) = if property.is_collection() {
            let convert = matches!(ptype, PropertyType::String | PropertyType::Reference);
            (
                quote! { std::vec::Vec<#prop_type> },
                PropertyType::Collection {
                    item: prop_type,
                    convert,
                },
            )
        } else {
            (prop_type, ptype)
        };
        Ok(Self {
            name: property.name(),
            prop_type,
            docs,
            required: property.required(),
            nullable: property.nullable(),
            ptype,
        })
    }
}

impl PropertyDef<'_> {
    pub const fn is_odata_type(&self) -> bool {
        matches!(self.name, PropertyName::Annotation(Annotation::ODataType))
    }

    pub const fn constructor_required(&self) -> bool {
        *self.required.inner() && !self.is_odata_type()
    }

    pub fn stored_type(&self) -> Cow<'_, TokenStream> {
        let value = &self.prop_type;
        match (
            self.required.into_inner(),
            self.nullable.into_inner(),
            &self.ptype,
        ) {
            (_, _, PropertyType::Additional) | (true, false, _) => Cow::Borrowed(value),
            (true, true, _) | (false, false, _) => {
                Cow::Owned(quote! { std::option::Option<#value> })
            }
            (false, true, _) => {
                Cow::Owned(quote! { std::option::Option<std::option::Option<#value>> })
            }
        }
    }

    pub fn definition(&self, serialize: bool) -> TokenStream {
        let name = &self.name;
        let ty = self.stored_type();
        let docs = &self.docs;
        let attributes = if serialize {
            match self.name.rename() {
                None => quote! { #[serde(flatten)] },
                Some(wire) if self.required.into_inner() => quote! { #[serde(rename = #wire)] },
                Some(wire) => {
                    quote! { #[serde(rename = #wire, skip_serializing_if = "std::option::Option::is_none")] }
                }
            }
        } else {
            quote! {}
        };
        quote! { #docs #attributes pub #name: #ty, }
    }

    pub fn argument_type(&self) -> Cow<'_, TokenStream> {
        let ty = &self.prop_type;
        match &self.ptype {
            PropertyType::String | PropertyType::Reference => {
                Cow::Owned(quote! { impl ::std::convert::Into<#ty> })
            }
            PropertyType::Collection {
                item,
                convert: true,
            } => Cow::Owned(quote! {
                impl ::std::iter::IntoIterator<Item = impl ::std::convert::Into<#item>>
            }),
            PropertyType::Collection {
                item,
                convert: false,
            } => Cow::Owned(quote! { impl ::std::iter::IntoIterator<Item = #item> }),
            _ => Cow::Borrowed(ty),
        }
    }

    pub fn convert<'a>(&self, value: &'a TokenStream) -> Cow<'a, TokenStream> {
        match &self.ptype {
            PropertyType::String | PropertyType::Reference => Cow::Owned(quote! { #value.into() }),
            PropertyType::Collection { convert: true, .. } => {
                Cow::Owned(quote! { #value.into_iter().map(::std::convert::Into::into).collect() })
            }
            PropertyType::Collection { convert: false, .. } => {
                Cow::Owned(quote! { #value.into_iter().collect() })
            }
            _ => Cow::Borrowed(value),
        }
    }

    pub fn setter(&self) -> TokenStream {
        let name = &self.name;
        let ty = self.argument_type();
        let docs = &self.docs;
        let value = quote! { value };
        let value = self.convert(&value);
        let value = match (
            self.required.into_inner(),
            self.nullable.into_inner(),
            &self.ptype,
        ) {
            (_, _, PropertyType::Additional) | (true, false, _) => value,
            (true, true, _) | (false, false, _) => Cow::Owned(quote! { Some(#value) }),
            (false, true, _) => Cow::Owned(quote! { Some(Some(#value)) }),
        };
        quote! {
            #docs
            pub fn #name(mut self, value: #ty) -> Self {
                self.#name = #value;
                self
            }
        }
    }
}
