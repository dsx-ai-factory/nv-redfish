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
use crate::compiler::NavProperty;
use crate::compiler::OData;
use crate::compiler::Parameter;
use crate::compiler::ParameterType;
use crate::compiler::Properties;
use crate::compiler::Property;
use crate::compiler::PropertyType;
use crate::compiler::QualifiedName;
use crate::compiler::RigidArraySupport;
use crate::generator::rust::doc::format_and_generate as doc_format_and_generate;
use crate::generator::rust::doc::format_and_generate_with_deprecation as doc_format_deprecated;
use crate::generator::rust::inherited_properties::InheritedProperties;
use crate::generator::rust::ActionFullTypeName;
use crate::generator::rust::ActionName;
use crate::generator::rust::Config;
use crate::generator::rust::Error;
use crate::generator::rust::FullTypeName;
use crate::generator::rust::SerializableProperties;
use crate::generator::rust::StructFieldName;
use crate::generator::rust::TypeName;
use crate::odata::annotations::Permissions;
use crate::redfish::DynamicProperties;
use crate::redfish::ExcerptCopy;
use crate::IsNullable;
use crate::IsRequired;
use crate::OneOrCollection;
use proc_macro2::Ident;
use proc_macro2::Literal;
use proc_macro2::Span;
use proc_macro2::TokenStream;
use quote::quote;
use quote::ToTokens;
use std::collections::HashSet;
use std::iter::once;

#[derive(Debug)]
pub enum GenerateType<'a> {
    Read,
    Excerpt(&'a ExcerptCopy),
    Update,
    Create,
    Action,
}

/// Generation of Rust struct.
#[derive(Debug)]
pub struct StructDef<'a> {
    pub name: TypeName<'a>,
    properties: Vec<&'a Properties<'a>>,
    parameters: &'a [Parameter<'a>],
    actions: Vec<&'a Action<'a>>,
    odata: OData<'a>,
    generate: Vec<GenerateType<'a>>,
    create_type: Option<QualifiedName<'a>>,
    need_redfish_settings: bool,
    dynamic_properties: Option<DynamicProperties<'a>>,
}

#[derive(Clone, Copy)]
enum SerializableStructKind {
    Create,
    Update,
}

// Action request fields are generated as two coordinated token streams. Keeping
// them in one value makes it harder to change the serde omission rule without
// also considering the generated Rust field type.
struct ActionParameterField {
    serde_annotation: TokenStream,
    field_type: TokenStream,
}

impl<'a> StructDef<'a> {
    /// Create `StructDef` builder.
    #[must_use]
    pub fn builder(name: TypeName<'a>, odata: OData<'a>) -> StructDefBuilder<'a> {
        StructDefBuilder::new(name, odata)
    }

    /// Generate rust code for the structure.
    pub fn generate(self, tokens: &mut TokenStream, config: &Config) {
        for t in &self.generate {
            match t {
                GenerateType::Create => self.generate_create(tokens, config),
                GenerateType::Read => self.generate_read(tokens, config),
                GenerateType::Excerpt(v) => self.generate_excerpt(tokens, config, v),
                GenerateType::Update => self.generate_update(tokens, config),
                GenerateType::Action => self.generate_action(tokens, config),
            }
        }
    }

    fn generate_read(&self, tokens: &mut TokenStream, config: &Config) {
        let top = &config.top_module_alias;
        let mut content = self.read_fields(config);
        let odata_id = Ident::new("odata_id", Span::call_site());
        let odata_etag = Ident::new("odata_etag", Span::call_site());

        let additional_properties = self.additional_properties_field(config);
        content.extend(additional_properties);

        let name = self.name;

        let serialize = config
            .serialize_read_models
            .then(|| quote! { #[derive(Serialize)] });

        // Explicit implementations avoid trait-solver recursion through the resource tree.
        // Every generated field is composed of Send + Sync schema types and core wrappers.
        tokens.extend([
            doc_format_and_generate(self.name, &self.odata),
            quote! {
                #serialize
                #[derive(Deserialize, Debug)]
                pub struct #name { #content }
                #[doc = "SAFETY: All generated data types are Send"]
                unsafe impl Send for #name {}
                #[doc = "SAFETY: All generated data types are Sync"]
                unsafe impl Sync for #name {}
            },
        ]);

        if self.odata.must_have_id.into_inner() {
            tokens.extend(quote! {
                impl #top::EntityTypeRef for #name {
                    #[inline] fn odata_id(&self) -> &ODataId { &self.#odata_id }
                    #[inline] fn etag(&self) -> Option<&ODataETag> { self.#odata_etag.as_ref() }
                }
            });
            self.generate_entity_type_traits(tokens, config);
        }

        if !self.actions.is_empty() {
            let mut content = TokenStream::new();
            for a in &self.actions {
                Self::generate_action_function(&mut content, a, config);
            }
            tokens.extend(quote! {
                impl #name { #content }
            });
        }
    }

    fn read_fields(&self, config: &Config) -> TokenStream {
        // Properties token streams:
        let properties_iter = self
            .read_properties()
            .map(|p| Self::generate_property(p, config));

        // Navigation properties token streams:
        let nav_properties_iter = self
            .read_nav_properties()
            .map(|p| Self::generate_nav_property(p, config));

        // Action properties token streams:
        let action_iter = self
            .actions
            .iter()
            .map(|a| Self::generate_action_property(a, config));

        once(self.metadata_fields(config))
            .chain(properties_iter)
            .chain(nav_properties_iter)
            .chain(action_iter)
            .collect()
    }

    fn read_properties(&self) -> impl Iterator<Item = &Property<'a>> {
        self.properties
            .iter()
            .flat_map(|p| &p.properties)
            .filter(|p| {
                !p.odata.permissions_is_write_only() && !p.redfish.is_excerpt_only.into_inner()
            })
    }

    fn read_nav_properties(&self) -> impl Iterator<Item = &NavProperty<'a>> {
        self.properties
            .iter()
            .flat_map(|p| &p.nav_properties)
            .filter(
                |p| !matches!(p, NavProperty::Expandable(p) if p.odata.permissions_is_write_only()),
            )
    }

    fn additional_properties_field(&self, config: &Config) -> TokenStream {
        let top = &config.top_module_alias;
        if self.odata.additional_properties.is_some_and(|v| *v.inner()) {
            // If additional_properties are explicitly set then we add
            // placeholder with serde_json::Value to
            // deserializer. Actually, it is almost always Oem /
            // OemAction.
            quote! {
                #[serde(flatten)]
                pub additional_properties: #top::AdditionalProperties,
            }
        } else {
            // Add dynamic properties if no additional properties
            // defined.
            self.dynamic_properties.map_or_else(TokenStream::new, |v| {
                let dynamic_type = Self::dynamic_properties_type(v, config);
                quote! {
                    #[serde(flatten)]
                    pub dynamic_properties: #dynamic_type,
                }
            })
        }
    }

    fn generate_excerpt(
        &self,
        tokens: &mut TokenStream,
        config: &Config,
        excerpt_copy: &ExcerptCopy,
    ) {
        let mut content = TokenStream::new();
        let all_properties = self
            .properties
            .iter()
            .flat_map(|p| &p.properties)
            .filter_map(|p| {
                if !p.odata.permissions_is_write_only()
                    && p.redfish
                        .excerpt
                        .as_ref()
                        .is_some_and(|excerpt| excerpt.matches(excerpt_copy))
                {
                    Some(Self::generate_property(p, config))
                } else {
                    None
                }
            });

        content.extend(all_properties);

        let name = self.name.for_excerpt_copy(excerpt_copy);

        let serialize = config
            .serialize_read_models
            .then(|| quote! { #[derive(Serialize)] });

        tokens.extend([quote! {
            #serialize
            #[derive(Deserialize, Debug)]
            pub struct #name { #content }
        }]);
    }

    fn metadata_fields(&self, config: &Config) -> TokenStream {
        let top = &config.top_module_alias;
        let mut fields = TokenStream::new();
        if self.odata.must_have_id.into_inner() {
            fields.extend(quote! {
                #[serde(rename = "@odata.id")]
                pub odata_id: ODataId,
                #[serde(rename = "@odata.etag", skip_serializing_if = "Option::is_none")]
                pub odata_etag: Option<ODataETag>,
            });
        }
        if self.odata.must_have_type.into_inner() {
            fields.extend(quote! {
                /// Type of the resource.
                #[serde(rename = "@odata.type")]
                pub odata_type: String,
            });
        }
        if self.odata.must_have_id.into_inner() {
            fields.extend(quote! {
                /// Settings annotations.
                #[serde(flatten)]
                pub settings_annotations: #top::SettingsAnnotations,
            });
        }
        fields
    }

    fn generate_update(&self, tokens: &mut TokenStream, config: &Config) {
        let properties = SerializableProperties::for_update(&self.properties, config);

        let has_additional_properties =
            self.odata.additional_properties.is_some_and(|v| *v.inner());
        let additional_properties = if has_additional_properties {
            let top = &config.top_module_alias;
            // If additional_properties are explicitly set then we add
            // placeholder with serde_json::Value to
            // serde_json. Actually, it is almost always Oem.
            quote! {
                #[serde(flatten)]
                pub additional_properties: #top::AdditionalProperties,
            }
        } else {
            TokenStream::new()
        };
        let dynamic_properties_type = if has_additional_properties {
            None
        } else {
            self.dynamic_properties
                .map(|v| Self::dynamic_properties_type(v, config))
        };
        let dynamic_properties = dynamic_properties_type.as_ref().map(|dynamic_type| {
            quote! {
                #[serde(flatten)]
                pub dynamic_properties: #dynamic_type,
            }
        });
        let dynamic_properties_impl = dynamic_properties_type.as_ref().map(|dynamic_type| {
            quote! {
                #[must_use]
                pub fn with_dynamic_properties(mut self, v: #dynamic_type) -> Self {
                    self.dynamic_properties = v;
                    self
                }
            }
        });

        let content = properties.struct_content_for_update();
        let comment = format!(" Update struct corresponding to `{}`", self.name);
        let name = self.name.for_update(None);
        let (debug_derive, debug_impl) = Self::debug_serializable(
            &name,
            &properties,
            SerializableStructKind::Update,
            has_additional_properties,
            dynamic_properties_type.is_some(),
        );
        tokens.extend(quote! {
            #[doc = #comment]
            #[derive(Serialize, Default)]
            #debug_derive
            pub struct #name { #content #additional_properties #dynamic_properties }
        });

        let content = properties.optional_property_setter_for_update();

        // Generate builder for struct.
        tokens.extend(quote! {
            impl #name {
                #[must_use]
                pub fn builder() -> Self {
                    Self::default()
                }
                #[must_use]
                pub const fn build(self) -> Self {
                    self
                }
                #content
                #dynamic_properties_impl
            }
            #debug_impl
        });
    }

    fn generate_create(&self, tokens: &mut TokenStream, config: &Config) {
        let properties = SerializableProperties::for_create(&self.properties, config);
        let has_additional_properties =
            self.odata.additional_properties.is_some_and(|v| *v.inner());
        let top = &config.top_module_alias;
        let additional_properties = has_additional_properties.then(|| {
            quote! {
                #[serde(flatten)]
                pub additional_properties: #top::AdditionalProperties,
            }
        });
        let additional_properties_init = has_additional_properties.then(|| {
            quote! {
                additional_properties: #top::AdditionalProperties::default(),
            }
        });
        let content = properties.struct_content_for_create();
        let comment = format!(" Create struct corresponding to `{}`", self.name);
        let name = self.name.for_create();
        let (debug_derive, debug_impl) = Self::debug_serializable(
            &name,
            &properties,
            SerializableStructKind::Create,
            has_additional_properties,
            false,
        );
        tokens.extend([quote! {
            #[doc = #comment]
            #[derive(Serialize)]
            #debug_derive
            pub struct #name { #content #additional_properties }
        }]);

        let prop_fn_content = properties.optional_property_setter_for_create();
        // Implement builder for create struct:
        let builder_fn_arglist = properties.builder_fn_arg_list_for_create();
        let builder_fn_content = properties.builder_fn_content_for_create();

        tokens.extend([quote! {
            impl #name {
                #[must_use]
                pub fn builder(#builder_fn_arglist) -> Self {
                    Self {
                        #builder_fn_content
                        #additional_properties_init
                    }
                }
                #[must_use]
                pub fn build(self) -> Self {
                    self
                }
                #prop_fn_content
            }
            #debug_impl
        }]);
    }

    fn debug_serializable<N: ToTokens>(
        name: N,
        properties: &SerializableProperties<'a>,
        kind: SerializableStructKind,
        has_additional_properties: bool,
        has_dynamic_properties: bool,
    ) -> (TokenStream, TokenStream) {
        // Open properties contain arbitrary request values and have no schema metadata that can
        // identify sensitive entries, so always use a redacting Debug implementation for them.
        if properties.can_contain_sensitive_info()
            || has_additional_properties
            || has_dynamic_properties
        {
            let mut fields = TokenStream::new();
            match kind {
                SerializableStructKind::Update => {
                    properties
                        .debug_print_fields_for_update()
                        .to_tokens(&mut fields);
                }
                SerializableStructKind::Create => {
                    properties
                        .debug_print_fields_for_create()
                        .to_tokens(&mut fields);
                }
            }
            // Additional properties are arbitrary OEM-provided request values with no schema
            // metadata that can identify sensitive entries, so never expose their contents.
            let additional_properties = has_additional_properties
                .then(|| quote! { .field("additional_properties", &"<redacted>") });
            let dynamic_properties = has_dynamic_properties
                .then(|| quote! { .field("dynamic_properties", &"<redacted>") });
            (
                quote! {},
                quote! {
                    impl core::fmt::Debug for #name {
                        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                            f.debug_struct(stringify!(#name))
                                #fields
                                #additional_properties
                                #dynamic_properties
                                .finish()
                        }
                    }
                },
            )
        } else {
            (quote! { #[derive(Debug)] }, quote! {})
        }
    }

    fn generate_action(&self, tokens: &mut TokenStream, config: &Config) {
        let top = &config.top_module_alias;
        let mut content = TokenStream::new();
        content.extend(
            self.parameters
                .iter()
                .map(|p| Self::generate_action_parameter(p, config)),
        );

        let name = self.name;
        tokens.extend([
            doc_format_and_generate(self.name, &self.odata),
            // Action parameters can contain sensitive values such as passwords, keys, or
            // tokens. Redfish CSDL schemas do not reliably identify which action parameters
            // are sensitive, so generating a safe Debug implementation requires explicit
            // sensitivity metadata. Do not derive Debug until those fields can be redacted.
            quote! {
                #[derive(Serialize)]
                pub struct #name {
                    /// Protocol annotations for this action request.
                    #[serde(flatten)]
                    pub redfish_annotations: #top::ActionAnnotations,
                    #content
                }
            },
        ]);
    }

    fn generate_property(p: &Property<'_>, config: &Config) -> TokenStream {
        let doc = doc_format_deprecated(p.name, &p.odata, p.redfish.deprecation);
        let (serde, field_type) = Self::gen_de_struct_field(
            &p.ptype,
            FullTypeName::new(p.ptype.name(), config),
            Literal::string(p.name.inner().inner()),
            p.nullable,
            p.redfish.is_required,
            p.rigid_array_support,
        );
        let name = StructFieldName::new_property(p.name);
        quote! { #doc #serde pub #name: #field_type, }
    }

    /// Returns the generated Rust map type for Redfish dynamic properties.
    fn dynamic_properties_type(
        dynamic_properties: DynamicProperties<'_>,
        config: &Config,
    ) -> TokenStream {
        let top = &config.top_module_alias;
        match dynamic_properties.ptype.as_str() {
            "Edm.PrimitiveType" => {
                quote! { #top::DynamicProperties<#top::edm::PrimitiveType> }
            }
            "Edm.String" => quote! { #top::DynamicProperties<#top::edm::String> },
            value => quote! { compile_error!(#value) },
        }
    }

    // Returns serde annotation and field type token streams.
    fn gen_de_struct_field<T>(
        cardinality: &OneOrCollection<T>,
        ftype: impl ToTokens,
        rename: impl ToTokens,
        nullable: IsNullable,
        required: IsRequired,
        rigid_array_support: RigidArraySupport,
    ) -> (TokenStream, TokenStream) {
        (
            Self::gen_de_struct_field_serde_annot(rename, nullable, required),
            Self::gen_de_struct_field_type(
                cardinality,
                ftype,
                nullable,
                required,
                rigid_array_support,
            ),
        )
    }

    fn gen_de_struct_field_serde_annot(
        rename: impl ToTokens,
        nullable: IsNullable,
        required: IsRequired,
    ) -> TokenStream {
        if required.into_inner() && nullable.into_inner() {
            quote! { #[serde(rename=#rename, deserialize_with="de_required_nullable")] }
        } else if required.into_inner() {
            quote! { #[serde(rename=#rename)] }
        } else if nullable.into_inner() {
            quote! {
                #[serde(
                    rename=#rename,
                    default,
                    deserialize_with="de_optional_nullable",
                    skip_serializing_if = "Option::is_none"
                )]
            }
        } else {
            quote! {
                #[serde(rename=#rename, default, skip_serializing_if = "Option::is_none")]
            }
        }
    }

    // Returns the Rust field type token stream.
    fn gen_de_struct_field_type<T>(
        cardinality: &OneOrCollection<T>,
        ftype: impl ToTokens,
        nullable: IsNullable,
        required: IsRequired,
        rigid_array_support: RigidArraySupport,
    ) -> TokenStream {
        match cardinality {
            OneOrCollection::One(_) => {
                if required.into_inner() && nullable.into_inner() {
                    quote! { Option<#ftype> }
                } else if required.into_inner() {
                    quote! { #ftype }
                } else if nullable.into_inner() {
                    quote! { Option<Option<#ftype>> }
                } else {
                    quote! { Option<#ftype> }
                }
            }
            OneOrCollection::Collection(_) => {
                let ftype = if rigid_array_support.into_inner() {
                    quote! { Option<#ftype> }
                } else {
                    quote! { #ftype }
                };

                if required.into_inner() && nullable.into_inner() {
                    quote! { Option<Vec<#ftype>> }
                } else if required.into_inner() {
                    quote! { Vec<#ftype> }
                } else if nullable.into_inner() {
                    quote! { Option<Option<Vec<#ftype>>> }
                } else {
                    quote! { Option<Vec<#ftype>>}
                }
            }
        }
    }

    fn generate_nav_property(p: &NavProperty<'_>, config: &Config) -> TokenStream {
        let name = StructFieldName::new_property(p.name());
        let rename = Literal::string(p.name().inner().inner());
        let (doc, serde, prop_type) = match p {
            NavProperty::Expandable(p) => {
                let doc = doc_format_deprecated(p.ptype.name(), &p.odata, p.redfish.deprecation);
                let ptype = p.redfish.excerpt_copy.as_ref().map_or_else(
                    || {
                        let full_type = FullTypeName::new(p.ptype.name(), config);
                        quote! { NavProperty<#full_type> }
                    },
                    |excerpt| {
                        FullTypeName::new(p.ptype.name(), config)
                            .for_excerpt_copy(excerpt)
                            .to_token_stream()
                    },
                );
                let (sa, t) = Self::gen_de_struct_field(
                    &p.ptype,
                    ptype,
                    rename,
                    p.nullable,
                    p.redfish.is_required,
                    RigidArraySupport::new(false),
                );
                (doc, sa, t)
            }
            NavProperty::Reference {
                cardinality,
                odata,
                redfish,
            } => {
                let doc = doc_format_deprecated(cardinality.inner(), odata, redfish.deprecation);
                let top = &config.top_module_alias;
                let (sa, t) = Self::gen_de_struct_field(
                    cardinality,
                    quote! { #top::ReferenceLeaf },
                    rename,
                    IsNullable::new(false),
                    IsRequired::new(false),
                    RigidArraySupport::new(false),
                );
                (doc, sa, t)
            }
        };
        quote! { #doc #serde pub #name: #prop_type, }
    }

    fn generate_action_parameter(p: &Parameter<'_>, config: &Config) -> TokenStream {
        let doc = doc_format_and_generate(p.name, &p.odata);
        let rename = Literal::string(p.name.inner().inner());
        let name = StructFieldName::new_parameter(p.name);
        let field = match p.ptype {
            ParameterType::Type(
                ptype
                @ (PropertyType::One((typeinfo, v)) | PropertyType::Collection((typeinfo, v))),
            ) => {
                if typeinfo.permissions.is_some_and(|p| p == Permissions::Read) {
                    return quote! {};
                }
                let full_type = FullTypeName::new(v, config).for_update(Some(typeinfo.class));
                Self::gen_action_parameter_field(&ptype, full_type, &rename, p.nullable, p.required)
            }
            ParameterType::Entity(e) => {
                let top = &config.top_module_alias;
                Self::gen_action_parameter_field(
                    &e,
                    quote! { #top::Reference },
                    &rename,
                    p.nullable,
                    p.required,
                )
            }
        };
        let serde = field.serde_annotation;
        let ptype = field.field_type;
        quote! {
            #doc
            #serde
            pub #name: #ptype,
        }
    }

    fn gen_action_parameter_field<T>(
        cardinality: &OneOrCollection<T>,
        ftype: impl ToTokens,
        rename: impl ToTokens,
        nullable: IsNullable,
        required: IsRequired,
    ) -> ActionParameterField {
        //
        // NOTE:
        //
        // Action request serialization depends on the generated Rust field
        // type and serde annotation agreeing on the same `required` and
        // `nullable` facts.
        //
        // `gen_de_struct_field_type` decides the Rust field type, such as
        // `Option<T>` or `Option<Option<T>>`, so nullability is encoded in that
        // type.
        //
        // `gen_action_parameter_serde_annotation` controls whether the field is
        // always present in the action request body, or omitted when the outer
        // `Option` is `None`. `skip_serializing_if = "Option::is_none"` is only
        // valid when the generated field type has an outer `Option`; otherwise,
        // a required field such as `Vec<T>` could be annotated with
        // `Option::is_none`.
        //
        // The non-obvious coordination is that both helpers branch on
        // `required`. In `gen_de_struct_field_type`, `required` generates a
        // non-`Option` type, or an `Option` whose `None` should serialize as
        // JSON `null`. That matches `gen_action_parameter_serde_annotation`:
        // required fields have no `skip_serializing_if`, while optional fields
        // omit outer `None`.
        //
        ActionParameterField {
            serde_annotation: Self::gen_action_parameter_serde_annotation(rename, required),
            field_type: Self::gen_de_struct_field_type(
                cardinality,
                ftype,
                nullable,
                required,
                RigidArraySupport::new(false),
            ),
        }
    }

    fn gen_action_parameter_serde_annotation(
        rename: impl ToTokens,
        required: IsRequired,
    ) -> TokenStream {
        if required.into_inner() {
            quote! { #[serde(rename=#rename)] }
        } else {
            quote! { #[serde(rename=#rename, skip_serializing_if = "Option::is_none")] }
        }
    }

    fn generate_action_property(a: &Action, config: &Config) -> TokenStream {
        let top = &config.top_module_alias;
        // Redfish serializes an action under its defining schema's
        // namespace ("#NvidiaChassis.Reset"), which for OEM actions
        // differs from the binding parameter's name.
        let rename = Literal::string(&format!("#{}.{}", a.defining_namespace, a.name));
        let name = ActionName::new(a.name);
        let typename =
            ActionFullTypeName::new(a.defining_namespace, a.binding_name, a.name, config);
        let ret_type = match a.return_type {
            Some(OneOrCollection::One(v)) => FullTypeName::new(v, config).to_token_stream(),
            Some(OneOrCollection::Collection(v)) => {
                let typename = FullTypeName::new(v, config);
                quote! { Vec<#typename> }
            }
            None => quote! { () },
        };
        quote! {
            #[serde(rename=#rename, skip_serializing_if = "Option::is_none")]
            pub #name: Option<#top::Action<#typename, #ret_type>>,
        }
    }

    fn generate_entity_type_traits(&self, tokens: &mut TokenStream, config: &Config) {
        let name = self.name;
        let top = &config.top_module_alias;
        tokens.extend(quote! {
            impl #top::Expandable for #name {}
        });
        let update_name = self.name.for_update(None);
        if self.odata.updatable.is_some_and(|v| v.inner().value) {
            tokens.extend(quote! {
                impl #top::Updatable<#update_name> for #name {}
            });
        }
        if self.need_redfish_settings {
            tokens.extend(quote! {
                impl #top::RedfishSettings<Self> for #name {
                    #[inline]
                    fn settings_object(&self) -> Option<NavProperty<Self>> {
                        self.settings_annotations.settings
                            .as_ref()
                            .and_then(|s| s.settings_object.as_ref())
                            .map(|r| NavProperty::Reference(r.into()))
                    }
                }
            });
        }
        if self.odata.deletable.is_some_and(|v| v.inner().value) {
            tokens.extend(quote! {
                impl #top::Deletable for #name {}
            });
        }

        if let Some(create_type) = self.create_type {
            let result_name = FullTypeName::new(create_type, config);
            let create_name = result_name.for_create();
            tokens.extend(quote! {
                impl #top::Creatable<#create_name, #result_name> for #name {}
            });
        }
    }

    fn generate_action_function(content: &mut TokenStream, a: &Action, config: &Config) {
        let top = &config.top_module_alias;
        let name = ActionName::new(a.name);
        let typename =
            ActionFullTypeName::new(a.defining_namespace, a.binding_name, a.name, config);
        let ret_type = match a.return_type {
            Some(OneOrCollection::One(v)) => FullTypeName::new(v, config).to_token_stream(),
            Some(OneOrCollection::Collection(v)) => {
                let typename = FullTypeName::new(v, config);
                quote! { Vec<#typename> }
            }
            None => quote! { () },
        };
        let doc_action_errors = quote! {
            #[doc = ""]
            #[doc = "# Errors"]
            #[doc = ""]
            #[doc = "* [Not supported error](nv_redfish_core::ActionError::not_supported) if reference to action is not supported by the server."]
            #[doc = "* [BMC Action errors](nv_redfish_core::Action::run) if returned by BMC implementation."]
        };
        if a.parameters.len() <= config.action_fn_max_param_number_threshold {
            let mut arglist = TokenStream::new();
            let mut params = TokenStream::new();
            for p in &a.parameters {
                let top = &config.top_module_alias;
                let name = StructFieldName::new_parameter(p.name);
                let argtype = match p.ptype {
                    ParameterType::Type(
                        ptype @ (PropertyType::One((typeinfo, v))
                        | PropertyType::Collection((typeinfo, v))),
                    ) => {
                        if typeinfo.permissions.is_some_and(|p| p == Permissions::Read) {
                            continue;
                        }
                        let full_type =
                            FullTypeName::new(v, config).for_update(Some(typeinfo.class));
                        Self::gen_de_struct_field_type(
                            &ptype,
                            full_type,
                            p.nullable,
                            p.required,
                            RigidArraySupport::new(false),
                        )
                    }
                    ParameterType::Entity(e) => {
                        let full_type = quote! { #top::Reference };
                        Self::gen_de_struct_field_type(
                            &e,
                            full_type,
                            p.nullable,
                            p.required,
                            RigidArraySupport::new(false),
                        )
                    }
                };
                params.extend(quote! { #name, });
                arglist.extend(quote! {, #name: #argtype });
            }
            content.extend([
                doc_format_and_generate(a.name, &a.odata),
                doc_action_errors,
                quote! {
                    pub async fn #name<B: #top::Bmc>(&self, bmc: &B #arglist) -> Result<nv_redfish_core::ModificationResponse<#ret_type>, B::Error>
                    where B::Error: #top::ActionError,
                    {
                        if let Some(a) = &self.#name  {
                            a.run(bmc, &#typename {
                                redfish_annotations: #top::ActionAnnotations::default(),
                                #params
                            }).await
                        } else {
                            Err(B::Error::not_supported())
                        }
                    }
                },
            ]);
        } else {
            content.extend([
                doc_format_and_generate(a.name, &a.odata),
                doc_action_errors,
                quote! {
                    pub async fn #name<B: #top::Bmc>(&self, bmc: &B, t: &#typename) -> Result<nv_redfish_core::ModificationResponse<#ret_type>, B::Error>
                    where B::Error: #top::ActionError,
                    {
                        if let Some(a) = &self.#name  {
                            a.run(bmc, t).await
                        } else {
                            Err(B::Error::not_supported())
                        }
                    }
                },
            ]);
        }
    }
}

/// Builder of the `StructDef`
pub struct StructDefBuilder<'a>(StructDef<'a>);

impl<'a> StructDefBuilder<'a> {
    #[must_use]
    fn new(name: TypeName<'a>, odata: OData<'a>) -> Self {
        Self(StructDef {
            name,
            properties: Vec::new(),
            parameters: &[],
            actions: Vec::new(),
            odata,
            generate: vec![GenerateType::Read],
            create_type: None,
            need_redfish_settings: false,
            dynamic_properties: None,
        })
    }

    /// Include inherited fields directly in each generated model.
    #[must_use]
    pub fn with_inherited_properties(mut self, inherited: InheritedProperties<'a>) -> Self {
        self.0.properties = inherited.properties;
        self.0.actions = inherited.actions;
        self.0.odata.must_have_type = inherited.must_have_type;
        self.0.odata.additional_properties = inherited.additional_properties;
        self.0.dynamic_properties = inherited.dynamic_properties;
        self
    }

    /// Setup parameters for the struct (for action structs).
    #[must_use]
    pub const fn with_parameters(mut self, parameters: &'a [Parameter<'a>]) -> Self {
        self.0.parameters = parameters;
        self
    }

    /// Setup create type for the struct.
    #[must_use]
    pub const fn with_create(mut self, ct: QualifiedName<'a>) -> Self {
        self.0.create_type = Some(ct);
        self
    }

    /// Setup generation types for the struct.
    #[must_use]
    pub fn with_generate_type(mut self, generate: Vec<GenerateType<'a>>) -> Self {
        self.0.generate = generate;
        self
    }

    /// Generate the typed settings resource accessor.
    #[must_use]
    pub const fn with_redfish_settings(mut self) -> Self {
        self.0.need_redfish_settings = true;
        self
    }

    /// Add support of dynamic properties.
    #[must_use]
    pub const fn with_dynamic_properties(mut self, dp: DynamicProperties<'a>) -> Self {
        self.0.dynamic_properties = Some(dp);
        self
    }

    /// # Errors
    ///
    /// Returns error if struct definition cannot be generated by the
    /// provided parameters.
    pub fn build(self, _config: &Config) -> Result<StructDef<'a>, Error<'a>> {
        let mut names = HashSet::new();
        for properties in &self.0.properties {
            for name in properties
                .properties
                .iter()
                .map(|p| p.name)
                .chain(properties.nav_properties.iter().map(NavProperty::name))
            {
                if !names.insert(StructFieldName::new_property(name)) {
                    return Err(Error::NameConflict);
                }
            }
        }
        Ok(self.0)
    }
}

#[cfg(test)]
mod tests;
