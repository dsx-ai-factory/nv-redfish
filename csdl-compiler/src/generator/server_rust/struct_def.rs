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

use std::borrow::Cow;
use std::collections::BTreeSet;

use proc_macro2::TokenStream;
use quote::quote;

use super::{
    property::{self, Property},
    property_def::{PropertyDef, PropertyType},
    types, Config, Error,
};
use crate::compiler::{Compiled, OData, QualifiedName};
use crate::generator::common_rust::{
    doc::format_and_generate, ident::escaped, InheritedProperties,
};
use crate::redfish::ExcerptCopy;

#[derive(Clone, Copy)]
pub(super) enum GenerateType<'a> {
    Response,
    Excerpt(&'a ExcerptCopy),
}

pub(super) struct StructDef<'a> {
    pub(super) name: types::ResponseTypeName<'a>,
    pub(super) helper: Option<types::RequiredTypeName<'a>>,
    properties: Vec<Property<'a>>,
    depth: usize,
    nearest_odata_with_description: Option<&'a OData<'a>>,
    odata_type: Option<QualifiedName<'a>>,
}

impl<'a> StructDef<'a> {
    pub(super) fn new(
        compiled: &'a Compiled<'a>,
        qname: QualifiedName<'a>,
        config: Config,
        generate: GenerateType<'a>,
    ) -> Self {
        let inherited = InheritedProperties::new(compiled, qname);
        let name = types::type_name(qname);
        let (name, properties, odata_type) = match generate {
            GenerateType::Response => (
                types::ResponseTypeName::Schema(name),
                property::collect(compiled, qname, &inherited),
                compiled
                    .entity_types
                    .contains_key(&qname)
                    .then(|| QualifiedName {
                        namespace: qname.namespace.original(),
                        ..qname
                    }),
            ),
            GenerateType::Excerpt(copy) => (
                types::ResponseTypeName::Excerpt(name.for_excerpt_copy(copy)),
                property::excerpt(compiled, &inherited, copy),
                None,
            ),
        };
        let required_count = properties
            .iter()
            .filter(|property| property.constructor_required())
            .count();
        let helper = (required_count > config.constructor_max_param_number_threshold)
            .then_some(types::RequiredTypeName(name));
        Self {
            name,
            helper,
            properties,
            depth: qname.namespace.len(),
            nearest_odata_with_description: inherited.nearest_odata_with_description,
            odata_type,
        }
    }

    pub(super) fn generate(self) -> Result<TokenStream, Error> {
        let Self {
            name,
            helper,
            properties,
            depth,
            nearest_odata_with_description,
            odata_type,
        } = self;
        let docs = nearest_odata_with_description.map(|odata| format_and_generate(name, odata));
        let properties = properties
            .into_iter()
            .map(|property| PropertyDef::new(property, depth))
            .collect::<Result<Vec<_>, _>>()?;
        validate_methods(&properties)?;
        let required: Vec<_> = properties
            .iter()
            .filter(|property| property.constructor_required())
            .collect();
        let helper_definition = helper.map(|helper| {
            let properties = required.iter().map(|property| property.definition(false));
            quote! {
                /// Mandatory properties supplied together to the resource constructor.
                #[derive(Debug)]
                pub struct #helper { #(#properties)* }
            }
        });
        let definitions = properties.iter().map(|property| property.definition(true));
        let setters = properties
            .iter()
            .filter(|property| !property.is_odata_type())
            .map(PropertyDef::setter);
        let null_setters = properties
            .iter()
            .filter(|property| property.nullable.into_inner())
            .map(null_setter);
        let constructor = constructor(&properties, helper.as_ref());
        let default = required
            .is_empty()
            .then(|| quote! { #[derive(::std::default::Default)] });
        let odata_type = odata_type.map(|source_name| {
            let value = format!("#{source_name}");
            quote! {
                /// The fully qualified CSDL type selected during generation.
                pub const ODATA_TYPE: &'static str = #value;
            }
        });
        Ok(quote! {
            #helper_definition
            #docs
            #default
            #[derive(Debug, serde::Serialize)]
            pub struct #name { #(#definitions)* }
            impl #name {
                #odata_type
                #constructor
                #(#setters)*
                #(#null_setters)*
            }
        })
    }
}

fn constructor(
    properties: &[PropertyDef<'_>],
    helper: Option<&types::RequiredTypeName<'_>>,
) -> TokenStream {
    let required = properties
        .iter()
        .filter(|property| property.constructor_required());
    let parameters = helper.map_or_else(
        || {
            let parameters = required.map(|property| {
                let name = &property.name;
                let ty = if property.nullable.into_inner() {
                    property.stored_type()
                } else {
                    property.argument_type()
                };
                quote! { #name: #ty, }
            });
            quote! { #(#parameters)* }
        },
        |helper| quote! { required: #helper },
    );
    let initializers = properties.iter().map(|property| {
        let name = &property.name;
        if property.constructor_required()
            && helper.is_none()
            && (property.nullable.into_inner() || matches!(property.ptype, PropertyType::Value))
        {
            return quote! { #name, };
        }
        let value = quote! { #name };
        let value = if property.is_odata_type() {
            Cow::Owned(quote! { Self::ODATA_TYPE })
        } else if !property.required.into_inner() {
            if matches!(property.ptype, PropertyType::Additional) {
                Cow::Owned(quote! { ::std::default::Default::default() })
            } else {
                Cow::Owned(quote! { None })
            }
        } else if helper.is_some() {
            Cow::Owned(quote! { required.#name })
        } else {
            property.convert(&value)
        };
        quote! { #name: #value, }
    });
    quote! {
        /// Construct a model with its required properties.
        /// Optional properties are omitted until assigned.
        pub fn new(#parameters) -> Self {
            Self { #(#initializers)* }
        }
    }
}

fn null_setter(property: &PropertyDef<'_>) -> TokenStream {
    let property_name = &property.name;
    let name = escaped(&format!("{property_name}_null"));
    let value = if property.required.into_inner() {
        quote! { None }
    } else {
        quote! { Some(None) }
    };
    quote! {
        /// Serialize this nullable property explicitly as null.
        pub fn #name(mut self) -> Self {
            self.#property_name = #value;
            self
        }
    }
}

fn validate_methods(properties: &[PropertyDef<'_>]) -> Result<(), Error> {
    let mut names = BTreeSet::from([String::from("new")]);
    for property in properties {
        if !names.insert(property.name.to_string()) {
            return Err(Error::NameCollision(
                escaped(&property.name.to_string()).into(),
            ));
        }
        if property.nullable.into_inner() && !names.insert(format!("{}_null", property.name)) {
            return Err(Error::NameCollision(
                escaped(&format!("{}_null", property.name)).into(),
            ));
        }
    }
    Ok(())
}
