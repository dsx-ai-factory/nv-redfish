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

use super::{Builder, Config, Error, Input};
use crate::compiler::SchemaBundle;
use crate::edmx::{Edmx, QualifiedTypeName};

mod excerpts;

fn root(name: &str) -> QualifiedTypeName {
    name.parse().expect("valid qualified name in test fixture")
}

fn bundle(body: &str) -> Result<SchemaBundle, String> {
    let source = format!(
        r#"<edmx:Edmx Version="4.0"><edmx:DataServices>
      <Schema Namespace="Resource">
        <EntityType Name="Resource" Abstract="true"/>
        <EntityType Name="ResourceCollection" Abstract="true"/>
      </Schema>
      <Schema Namespace="RedfishExtensions.v1_0_0">
        <Term Name="Settings" Type="Settings.Settings"/>
        <Term Name="SettingsApplyTime" Type="Settings.PreferredApplyTime"/>
      </Schema>
      <Schema Namespace="Settings">
        <ComplexType Name="Settings"/><ComplexType Name="PreferredApplyTime"/>
      </Schema>
      {body}
    </edmx:DataServices></edmx:Edmx>"#
    );
    Ok(SchemaBundle::new(
        vec![Edmx::parse(&source).map_err(|e| e.to_string())?],
        Vec::new(),
    ))
}

#[test]
fn roots_follow_inheritance_and_keep_read_only_fields_and_docs() -> Result<(), String> {
    let bundle = bundle(
        r#"
      <Schema Namespace="Sample">
        <EntityType Name="Sample">
          <Property Name="Id" Type="Edm.String" Nullable="false"/>
        </EntityType>
      </Schema>
      <Schema Namespace="Sample.v1_0_0">
        <EntityType Name="Sample" BaseType="Sample.Sample">
          <Property Name="ReadOnlyValue" Type="Edm.Int64">
            <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Read"/>
            <Annotation Term="OData.Description" String="Documentation from CSDL."/>
          </Property>
        </EntityType>
      </Schema>
      <Schema Namespace="Sample.v1_1_0">
        <EntityType Name="Sample" BaseType="Sample.v1_0_0.Sample">
          <Property Name="AdditionalValue" Type="Edm.Boolean"/>
        </EntityType>
      </Schema>"#,
    )?;
    let generated = Builder::new(&[(&root("Sample.Sample")).into()])
        .generate(Input::Bundle(&bundle))
        .map_err(|e| e.to_string())?;
    assert!(generated.contains("pub fn id("));
    assert!(generated.contains("pub fn read_only_value("));
    assert!(generated.contains("pub fn additional_value("));
    assert!(generated.contains("Documentation from CSDL."));
    assert!(!generated.contains("pub mod v1_"));
    assert_eq!(generated.matches("pub struct Sample {").count(), 1);
    assert!(generated.contains("pub struct Resource {"));
    assert!(generated.contains("pub struct ResourceCollection {"));
    assert!(generated.contains("pub struct PreferredApplyTime {"));
    assert!(generated.contains("pub const ODATA_TYPE: &'static str = \"#Sample.v1_1_0.Sample\";"));
    assert!(generated.contains("odata_type: Self::ODATA_TYPE"));
    assert!(!generated.contains("pub fn odata_type("));
    Ok(())
}

#[test]
fn explicit_empty_entity_version_keeps_its_wire_type() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="Sample.v1_0_0">
             <EntityType Name="Sample"><Property Name="Value" Type="Edm.String"/></EntityType>
           </Schema>
           <Schema Namespace="Sample.v1_1_0">
             <EntityType Name="Sample" BaseType="Sample.v1_0_0.Sample"/>
           </Schema>"#,
    )?;
    let generated = Builder::new(&[(&root("Sample.v1_1_0.Sample")).into()])
        .generate(Input::Bundle(&bundle))
        .map_err(|e| e.to_string())?;
    assert!(generated.contains("pub const ODATA_TYPE: &'static str = \"#Sample.v1_1_0.Sample\";"));
    assert!(!generated.contains("\"#Sample.v1_0_0.Sample\""));
    assert!(generated.contains("pub fn value("));
    assert!(!generated.contains("pub mod v1_"));
    Ok(())
}

#[test]
fn schema_type_names_do_not_shadow_standard_traits() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="Sample">
             <ComplexType Name="Into"><Property Name="Label" Type="Edm.String"/></ComplexType>
             <ComplexType Name="IntoIterator">
               <Property Name="Values" Type="Collection(Edm.String)"/>
               <Property Name="Counts" Type="Collection(Edm.Int64)"/>
             </ComplexType>
             <ComplexType Name="Default"/>
           </Schema>"#,
    )?;
    let generated = Builder::new(&[
        (&root("Sample.Into")).into(),
        (&root("Sample.IntoIterator")).into(),
        (&root("Sample.Default")).into(),
    ])
    .generate(Input::Bundle(&bundle))
    .map_err(|e| e.to_string())?;
    assert!(generated.contains("impl ::std::convert::Into<"));
    assert!(generated.contains("impl ::std::iter::IntoIterator<"));
    assert!(generated.contains(".map(::std::convert::Into::into)"));
    assert!(generated.contains("#[derive(::std::default::Default)]"));
    assert!(!generated.contains("impl Into<"));
    assert!(!generated.contains("impl IntoIterator<"));
    Ok(())
}

#[test]
fn write_only_fields_are_omitted_before_constructor_and_count_generation() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="External"><EntityType Name="Target"/></Schema>
           <Schema Namespace="Sample">
             <EntityType Name="Base">
               <Property Name="InheritedSecret" Type="Edm.String" Nullable="false">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Write"/>
                 <Annotation Term="Redfish.Required"/>
               </Property>
               <Property Name="Overridden" Type="Edm.String"/>
               <Property Name="ReadOnlyValue" Type="Edm.String">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Read"/>
               </Property>
             </EntityType>
             <EntityType Name="Sample" BaseType="Sample.Base">
               <Property Name="Password" Type="Edm.String" Nullable="false">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Write"/>
                 <Annotation Term="Redfish.Required"/>
               </Property>
               <Property Name="Overridden" Type="Edm.String">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Write"/>
               </Property>
               <Property Name="ReadWriteValue" Type="Edm.String">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/ReadWrite"/>
               </Property>
               <NavigationProperty Name="HiddenReference" Type="Collection(External.Target)">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Write"/>
                 <Annotation Term="Redfish.Required"/>
               </NavigationProperty>
               <NavigationProperty Name="HiddenExpanded" Type="Collection(Sample.Sample)">
                 <Annotation Term="OData.Permissions" EnumMember="OData.Permission/Write"/>
                 <Annotation Term="Redfish.Required"/>
               </NavigationProperty>
               <NavigationProperty Name="VisibleReference" Type="External.Target"/>
             </EntityType>
           </Schema>"#,
    )?;
    let generated = Builder::new(&[(&root("Sample.Sample")).into()])
        .config(Config {
            constructor_max_param_number_threshold: 1,
        })
        .generate(Input::Bundle(&bundle))
        .map_err(|e| e.to_string())?;
    for name in [
        "password",
        "inherited_secret",
        "overridden",
        "hidden_reference",
        "hidden_expanded",
    ] {
        assert!(
            !generated.contains(name),
            "write-only field remains: {}",
            name
        );
    }
    assert!(!generated.contains("pub struct SampleRequired"));
    assert!(generated.contains("pub fn read_only_value("));
    assert!(generated.contains("pub fn read_write_value("));
    assert!(generated.contains("pub fn visible_reference("));
    Ok(())
}

#[test]
fn generated_type_does_not_count_toward_constructor_threshold() -> Result<(), String> {
    let bundle = bundle(r#"<Schema Namespace="Sample"><EntityType Name="Sample"/></Schema>"#)?;
    let generated = Builder::new(&[(&root("Sample.Sample")).into()])
        .config(Config {
            constructor_max_param_number_threshold: 1,
        })
        .generate(Input::Bundle(&bundle))
        .map_err(|e| e.to_string())?;
    assert!(!generated.contains("pub struct SampleRequired"));
    assert!(generated.contains("odata_id: impl ::std::convert::Into<std::string::String>"));
    assert!(generated.contains("pub const ODATA_TYPE: &'static str = \"#Sample.Sample\";"));
    Ok(())
}

#[test]
fn output_is_independent_of_root_order_and_duplicate_roots() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="Sample">
      <EntityType Name="First"/>
      <EntityType Name="Second"/>
    </Schema>"#,
    )?;
    let forward = Builder::new(&[
        (&root("Sample.First")).into(),
        (&root("Sample.Second")).into(),
    ])
    .generate(Input::Bundle(&bundle))
    .map_err(|e| e.to_string())?;
    let reverse = Builder::new(&[
        (&root("Sample.Second")).into(),
        (&root("Sample.First")).into(),
        (&root("Sample.First")).into(),
    ])
    .generate(Input::Bundle(&bundle))
    .map_err(|e| e.to_string())?;
    assert_eq!(forward, reverse);
    Ok(())
}

#[test]
fn typed_property_names_preserve_wire_names_and_rust_escaping() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="Sample"><ComplexType Name="Sample">
            <Property Name="Type" Type="Edm.String" Nullable="false"/>
            <Property Name="Self" Type="Edm.String" Nullable="false"/>
        </ComplexType></Schema>"#,
    )?;
    let generated = Builder::new(&[(&root("Sample.Sample")).into()])
        .generate(Input::Bundle(&bundle))
        .map_err(|e| e.to_string())?;
    assert!(generated.contains(r#"#[serde(rename = "Type", skip_serializing_if"#));
    assert!(generated.contains("pub r#type: std::option::Option<std::string::String>"));
    assert!(generated.contains("pub fn r#type("));
    assert!(generated.contains(r#"#[serde(rename = "Self", skip_serializing_if"#));
    assert!(generated.contains("pub fn self_("));
    Ok(())
}

#[test]
fn annotations_and_normalized_namespaces_still_detect_and_merge_rust_names() -> Result<(), String> {
    for property in [
        r#"<Property Name="OdataId" Type="Edm.String"/>"#,
        r#"<Property Name="MembersCount" Type="Edm.Int64"/>
           <NavigationProperty Name="Members" Type="Collection(Sample.Sample)"/>"#,
    ] {
        let bundle = bundle(&format!(
            r#"<Schema Namespace="Sample"><EntityType Name="Sample">{property}</EntityType></Schema>"#,
        ))?;
        assert!(matches!(
            Builder::new(&[(&root("Sample.Sample")).into()]).generate(Input::Bundle(&bundle)),
            Err(Error::NameCollision(_))
        ));
    }
    let bundle = bundle(
        r#"<Schema Namespace="FooBar"><ComplexType Name="First"/></Schema>
           <Schema Namespace="Foo_Bar"><ComplexType Name="Second"/></Schema>"#,
    )?;
    let generated = Builder::new(&[
        (&root("FooBar.First")).into(),
        (&root("Foo_Bar.Second")).into(),
    ])
    .generate(Input::Bundle(&bundle))
    .map_err(|e| e.to_string())?;
    assert_eq!(generated.matches("pub mod foo_bar {").count(), 1);
    assert!(generated.contains("pub struct First"));
    assert!(generated.contains("pub struct Second"));
    Ok(())
}

#[test]
fn normalized_namespaces_merge_before_checking_type_names() -> Result<(), String> {
    for second_name in ["Second", "First"] {
        let bundle = bundle(&format!(
            r#"<Schema Namespace="FooBar.Inner"><ComplexType Name="First"/></Schema>
               <Schema Namespace="Foo_Bar.Inner"><ComplexType Name="{second_name}"/></Schema>"#,
        ))?;
        let first = root("FooBar.Inner.First");
        let second = root(&format!("Foo_Bar.Inner.{second_name}"));
        let result =
            Builder::new(&[(&first).into(), (&second).into()]).generate(Input::Bundle(&bundle));
        if second_name == "First" {
            assert!(matches!(result, Err(Error::NameCollision(name)) if name.is_ident("First")));
        } else {
            let generated = result.map_err(|e| e.to_string())?;
            assert_eq!(generated.matches("pub mod foo_bar {").count(), 1);
            assert!(generated.contains("pub struct First"));
            assert!(generated.contains("pub struct Second"));
        }
    }
    Ok(())
}

#[test]
fn unsupported_primitives_unknown_roots_and_name_collisions_are_errors() -> Result<(), String> {
    for (source, expected) in [
        (
            r#"<Property Name="Location" Type="Edm.Geography"/>"#,
            "primitive",
        ),
        (
            r#"<Property Name="Name" Type="Edm.String"/><Property Name="NameNull" Type="Edm.String" Nullable="false"/>"#,
            "collision",
        ),
    ] {
        let bundle = bundle(&format!(
            r#"<Schema Namespace="Sample">
            <EntityType Name="Sample">{source}</EntityType></Schema>"#
        ))?;
        let error = Builder::new(&[(&root("Sample.Sample")).into()])
            .generate(Input::Bundle(&bundle))
            .err()
            .ok_or("expected generation to fail")?;
        assert!(error.to_string().contains(expected), "{}", error);
        match (&error, expected) {
            (Error::UnsupportedPrimitive(name), "primitive") => {
                assert_eq!(name, &root("Edm.Geography"));
            }
            (Error::NameCollision(path), "collision") => {
                assert!(path.is_ident("name_null"));
            }
            _ => panic!("unexpected error: {}", error),
        }
    }
    let bundle = bundle(r#"<Schema Namespace="Sample"><EntityType Name="Sample"/></Schema>"#)?;
    assert!(
        matches!(Builder::new(&[(&root("Sample.Absent")).into()]).generate(Input::Bundle(&bundle)), Err(Error::Compile(message)) if message.contains("Sample.Absent"))
    );
    Ok(())
}

#[test]
fn enum_named_string_is_still_a_closed_enum() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="Sample">
      <EnumType Name="String"><Member Name="OK"/><Member Name="IPBasedDrive"/></EnumType>
      <EntityType Name="Sample"><Property Name="Mode" Type="Sample.String" Nullable="false"/></EntityType>
    </Schema>"#,
    )?;
    let generated = Builder::new(&[(&root("Sample.Sample")).into()])
        .generate(Input::Bundle(&bundle))
        .map_err(|e| e.to_string())?;
    assert!(generated.contains("value: super::sample::String"));
    assert!(generated.contains("Ok,"));
    assert!(generated.contains("IpBasedDrive,"));
    assert!(generated.contains(r#"#[serde(rename = "OK")]"#));
    assert!(!generated.contains("UnsupportedValue"));
    Ok(())
}

#[test]
fn constructor_threshold_is_inclusive_and_configurable() -> Result<(), String> {
    for (threshold, count, helper) in [
        (3, 0, false),
        (3, 1, false),
        (3, 3, false),
        (3, 4, true),
        (0, 1, true),
        (4, 4, false),
    ] {
        let fields = (0..count)
            .map(|index| {
                format!(
                    r#"
            <Property Name="Value{index}" Type="Edm.String" Nullable="false">
              <Annotation Term="Redfish.Required"/>
            </Property>"#
                )
            })
            .collect::<String>();
        let bundle = bundle(&format!(
            r#"<Schema Namespace="Sample">
            <ComplexType Name="Sample">{fields}</ComplexType></Schema>"#
        ))?;
        let generated = Builder::new(&[(&root("Sample.Sample")).into()])
            .config(Config {
                constructor_max_param_number_threshold: threshold,
            })
            .generate(Input::Bundle(&bundle))
            .map_err(|e| e.to_string())?;
        assert_eq!(
            generated.contains("pub struct SampleRequired"),
            helper,
            "threshold={}, count={}",
            threshold,
            count
        );
    }
    Ok(())
}

#[test]
fn required_helper_names_cannot_shadow_schema_types() -> Result<(), String> {
    let bundle = bundle(
        r#"<Schema Namespace="Sample">
        <ComplexType Name="Sample">
          <Property Name="Name" Type="Edm.String" Nullable="false">
            <Annotation Term="Redfish.Required"/>
          </Property>
        </ComplexType>
        <ComplexType Name="SampleRequired"/>
    </Schema>"#,
    )?;
    assert!(
        matches!(Builder::new(&[(&root("Sample.Sample")).into(), (&root("Sample.SampleRequired")).into()]).config(Config { constructor_max_param_number_threshold: 0 }).generate(Input::Bundle(&bundle)), Err(Error::NameCollision(name)) if name.is_ident("SampleRequired"))
    );
    Ok(())
}
