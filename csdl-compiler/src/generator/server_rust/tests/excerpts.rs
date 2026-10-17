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

use super::{bundle, root, Builder, Config, Error, Input};
use crate::compiler::{Config as CompilerConfig, EntityTypeFilter, SchemaBundle};

fn excerpt_bundle() -> Result<SchemaBundle, String> {
    bundle(
        r#"<Schema Namespace="Sensor">
          <EntityType Name="Sensor">
            <Property Name="Id" Type="Edm.String" Nullable="false">
              <Annotation Term="Redfish.Required"/>
            </Property>
            <Property Name="Reading" Type="Edm.Decimal" Nullable="false">
              <Annotation Term="Redfish.Required"/>
              <Annotation Term="Redfish.Excerpt"/>
            </Property>
            <Property Name="DataSourceUri" Type="Edm.String" Nullable="false">
              <Annotation Term="Redfish.Required"/>
              <Annotation Term="Redfish.ExcerptCopyOnly"/>
            </Property>
            <Property Name="Overridden" Type="Edm.String">
              <Annotation Term="Redfish.Excerpt" String="Array"/>
            </Property>
            <Property Name="Secret" Type="Edm.String" Nullable="false">
              <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Write"/>
              <Annotation Term="Redfish.Required"/>
              <Annotation Term="Redfish.Excerpt"/>
            </Property>
            <Property Name="FalseCopyOnly" Type="Edm.String">
              <Annotation Term="Redfish.ExcerptCopyOnly" Bool="false"/>
            </Property>
            <Property Name="Unit" Type="Sensor.Unit">
              <Annotation Term="Redfish.Excerpt"/>
            </Property>
          </EntityType>
          <EnumType Name="Unit"><Member Name="Celsius"/></EnumType>
        </Schema>
        <Schema Namespace="Sensor.v1_0_0">
          <EntityType Name="Sensor" BaseType="Sensor.Sensor">
            <Property Name="Overridden" Type="Edm.String">
              <Annotation Term="Redfish.Excerpt" String="Single"/>
            </Property>
            <Property Name="DeviceName" Type="Edm.String">
              <Annotation Term="Redfish.ExcerptCopyOnly"/>
              <Annotation Term="Redfish.Excerpt" String="Array,FanArray"/>
            </Property>
          </EntityType>
        </Schema>
        <Schema Namespace="Metrics">
          <EntityType Name="Metrics">
            <NavigationProperty Name="Internal" Type="Sensor.Sensor" Nullable="false">
              <Annotation Term="Redfish.Required"/>
              <Annotation Term="Redfish.ExcerptCopy" String="Single"/>
            </NavigationProperty>
            <NavigationProperty Name="Readings" Type="Collection(Sensor.Sensor)" Nullable="true">
              <Annotation Term="Redfish.ExcerptCopy" String="Array"/>
            </NavigationProperty>
            <NavigationProperty Name="Summary" Type="Sensor.Sensor" Nullable="false">
              <Annotation Term="Redfish.ExcerptCopy"/>
            </NavigationProperty>
            <NavigationProperty Name="Sensor" Type="Sensor.Sensor" Nullable="false"/>
            <NavigationProperty Name="Sensors" Type="Collection(Sensor.Sensor)" Nullable="false"/>
            <Property Name="Details" Type="Metrics.Details" Nullable="false"/>
          </EntityType>
          <ComplexType Name="Details">
            <NavigationProperty Name="Last" Type="Sensor.Sensor" Nullable="true">
              <Annotation Term="Redfish.ExcerptCopy" String="Single"/>
            </NavigationProperty>
          </ComplexType>
        </Schema>"#,
    )
}

fn generate(config: Config) -> Result<String, String> {
    Builder::new(&[(&root("Metrics.Metrics")).into()])
        .config(config)
        .generate(Input::Bundle(&excerpt_bundle()?))
        .map_err(|error| error.to_string())
}

fn model<'a>(syntax: &'a syn::File, module: &str, name: &str) -> &'a syn::ItemStruct {
    syntax
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Mod(item) if item.ident == module => {
                item.content.as_ref().and_then(|(_, items)| {
                    items.iter().find_map(|item| match item {
                        syn::Item::Struct(item) if item.ident == name => Some(item),
                        _ => None,
                    })
                })
            }
            _ => None,
        })
        .expect("expected model was generated in the selected namespace")
}

fn fields(model: &syn::ItemStruct) -> Vec<String> {
    model
        .fields
        .iter()
        .map(|field| {
            field
                .ident
                .as_ref()
                .expect("generated struct has named fields")
                .to_string()
        })
        .collect()
}

fn assert_field_type(model: &syn::ItemStruct, name: &str, expected: &syn::Type) {
    // Normalize layout and optional trailing commas in generic arguments.
    fn formatted(ty: &syn::Type) -> String {
        prettyplease::unparse(&syn::parse_quote!(type Field = #ty;))
    }

    let actual = &model
        .fields
        .iter()
        .find(|field| field.ident.as_ref().is_some_and(|ident| ident == name))
        .expect("expected field was generated in the model")
        .ty;
    assert_eq!(
        formatted(actual),
        formatted(expected),
        "{}::{}",
        model.ident,
        name
    );
}

#[test]
fn excerpt_fields_follow_keys_inheritance_and_copy_only_annotations() -> Result<(), String> {
    let generated = generate(Config::default())?;
    let syntax = syn::parse_file(&generated).map_err(|error| error.to_string())?;
    assert_eq!(
        fields(model(&syntax, "sensor", "Sensor")),
        [
            "odata_id",
            "odata_type",
            "odata_etag",
            "odata_context",
            "id",
            "reading",
            "overridden",
            "false_copy_only",
            "unit",
        ]
    );
    for (name, expected) in [
        (
            "SensorExcerpt",
            vec![
                "reading",
                "data_source_uri",
                "overridden",
                "unit",
                "device_name",
            ],
        ),
        (
            "SensorExcerptArray",
            vec!["reading", "data_source_uri", "unit", "device_name"],
        ),
        (
            "SensorExcerptSingle",
            vec!["reading", "data_source_uri", "overridden", "unit"],
        ),
    ] {
        let excerpt = model(&syntax, "sensor", name);
        assert_eq!(fields(excerpt), expected, "{name}");
        assert_field_type(
            excerpt,
            "unit",
            &syn::parse_quote!(std::option::Option<std::option::Option<super::sensor::Unit>>),
        );
    }
    assert!(generated.contains("pub enum Unit"));
    assert!(!generated.contains("pub fn secret("));
    Ok(())
}

#[test]
fn excerpt_navigation_uses_models_and_only_links_have_counts() -> Result<(), String> {
    let generated = generate(Config::default())?;
    let syntax = syn::parse_file(&generated).map_err(|error| error.to_string())?;
    let metrics = model(&syntax, "metrics", "Metrics");
    assert_field_type(
        metrics,
        "internal",
        &syn::parse_quote!(super::sensor::SensorExcerptSingle),
    );
    assert_field_type(
        metrics,
        "readings",
        &syn::parse_quote!(
            std::option::Option<
                std::option::Option<std::vec::Vec<super::sensor::SensorExcerptArray>>,
            >
        ),
    );
    assert_field_type(
        metrics,
        "summary",
        &syn::parse_quote!(std::option::Option<super::sensor::SensorExcerpt>),
    );
    assert_field_type(
        metrics,
        "sensor",
        &syn::parse_quote!(std::option::Option<nv_redfish_server_core::Reference>),
    );
    assert_field_type(
        metrics,
        "sensors",
        &syn::parse_quote!(std::option::Option<std::vec::Vec<nv_redfish_server_core::Reference>>),
    );
    assert!(fields(metrics).contains(&String::from("sensors_count")));
    assert!(!fields(metrics).contains(&String::from("readings_count")));
    assert_field_type(
        model(&syntax, "metrics", "Details"),
        "last",
        &syn::parse_quote!(
            std::option::Option<std::option::Option<super::sensor::SensorExcerptSingle>>
        ),
    );
    assert!(generated.contains("Item = super::sensor::SensorExcerptArray"));
    assert!(generated.contains("pub fn readings_null("));
    Ok(())
}

#[test]
fn excerpt_required_helpers_and_order_are_stable() -> Result<(), String> {
    let config = Config {
        constructor_max_param_number_threshold: 1,
    };
    let generated = generate(config)?;
    assert_eq!(generated, generate(config)?);
    let syntax = syn::parse_file(&generated).map_err(|error| error.to_string())?;
    for name in [
        "SensorExcerptRequired",
        "SensorExcerptArrayRequired",
        "SensorExcerptSingleRequired",
    ] {
        assert_eq!(
            fields(model(&syntax, "sensor", name)),
            ["reading", "data_source_uri"]
        );
    }
    assert!(generated.contains("pub fn new(required: SensorExcerptSingleRequired)"));
    assert!(generated.contains("pub fn new(required: SensorExcerptArrayRequired)"));
    Ok(())
}

#[test]
fn excerpt_names_and_required_helpers_participate_in_collision_checks() -> Result<(), String> {
    for name in ["SensorExcerptSingle", "SensorExcerptSingleRequired"] {
        let bundle = bundle(&format!(
            r#"
            <Schema Namespace="Sensor">
              <EntityType Name="Sensor">
                <Property Name="Reading" Type="Edm.Decimal" Nullable="false">
                  <Annotation Term="Redfish.Required"/>
                  <Annotation Term="Redfish.Excerpt"/>
                </Property>
              </EntityType>
              <ComplexType Name="{name}"/>
            </Schema>
            <Schema Namespace="Metrics">
              <EntityType Name="Metrics">
                <Property Name="Conflict" Type="Sensor.{name}" Nullable="false"/>
                <NavigationProperty Name="Internal" Type="Sensor.Sensor">
                  <Annotation Term="Redfish.ExcerptCopy" String="Single"/>
                </NavigationProperty>
              </EntityType>
            </Schema>"#
        ))?;
        let result = Builder::new(&[
            (&root("Metrics.Metrics")).into(),
            (&root(&format!("Sensor.{name}"))).into(),
        ])
        .config(Config {
            constructor_max_param_number_threshold: 0,
        })
        .generate(Input::Bundle(&bundle));
        assert!(
            matches!(result, Err(Error::NameCollision(path)) if path.is_ident(name)),
            "{}",
            name
        );
    }
    Ok(())
}

#[test]
fn client_entity_filter_still_excludes_unselected_excerpt_dependencies() -> Result<(), String> {
    let bundle = excerpt_bundle()?;
    let (compiled, _) = bundle
        .compile_named_types(
            &[(&root("Metrics.Metrics")).into()],
            CompilerConfig {
                entity_type_filter: EntityTypeFilter::new_restrictive(Vec::new()),
                ..CompilerConfig::default()
            },
        )
        .map_err(|error| error.to_string())?;
    assert!(compiled.excerpt_copies.is_empty());
    assert!(!compiled
        .entity_types
        .keys()
        .any(|name| name.name.inner() == "Sensor"));
    Ok(())
}
