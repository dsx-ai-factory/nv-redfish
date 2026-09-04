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

use nv_redfish_csdl_compiler::compiler::Config;
use nv_redfish_csdl_compiler::compiler::EntityTypeFilter;
use nv_redfish_csdl_compiler::compiler::NavProperty;
use nv_redfish_csdl_compiler::compiler::NavPropertyExpandable;
use nv_redfish_csdl_compiler::compiler::NavPropertyType;
use nv_redfish_csdl_compiler::compiler::PropertyType;
use nv_redfish_csdl_compiler::compiler::SchemaBundle;
use nv_redfish_csdl_compiler::edmx::attribute_values::Error as AttributeValuesError;
use nv_redfish_csdl_compiler::edmx::Edmx;
use nv_redfish_csdl_compiler::edmx::ValidateError;
use nv_redfish_csdl_compiler::optimizer::optimize;
use nv_redfish_csdl_compiler::optimizer::Config as OptimizerConfig;
use nv_redfish_csdl_compiler::OneOrCollection;
use std::io::Error as IoError;
use std::io::Read;

#[allow(dead_code)]
#[derive(Debug)]
enum Error {
    ParameterNeeded,
    Io(String, IoError),
    Edmx(String, ValidateError),
    Compile(String),
    WrongRootService(AttributeValuesError),
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("Usage:");
        println!(" {} <root service> <redfish-csdl-file> ...", args[0]);
        return Err(Error::ParameterNeeded);
    }

    let root_service = args[1].parse().map_err(Error::WrongRootService)?;

    let root_docs = args
        .iter()
        .skip(2)
        .map(|fname| {
            let mut file =
                std::fs::File::open(fname).map_err(|err| Error::Io(fname.clone(), err))?;

            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|err| Error::Io(fname.clone(), err))?;

            Edmx::parse(&content).map_err(|e| Error::Edmx(fname.clone(), e))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let schema_bundle = SchemaBundle::new(root_docs, Vec::new());

    let compiled = schema_bundle
        .compile(
            &[root_service],
            &EntityTypeFilter::new_restrictive(vec![]),
            Config::default(),
        )
        .inspect_err(|e| println!("{e}"))
        .map_err(|_| Error::Compile("compilation error".into()))?;
    let compiled = optimize(compiled, &OptimizerConfig::default());

    println!("Enum types:");
    for t in compiled.enum_types.values() {
        print!("  {}: ", t.name);
        println!("Enum ({t:?})");
    }
    println!();
    println!("Type definitions:");
    for t in compiled.type_definitions.values() {
        print!("  {}: ", t.name);
        println!("Typedef ({t:?})");
    }
    println!();
    println!("Complex types:");
    for t in compiled.complex_types.values() {
        print!("  {}", t.name);
        if let Some(base) = t.base {
            print!(" extends {base}");
        }
        println!();
        if !t.properties.is_empty() {
            println!("    properties:");
            for p in &t.properties.properties {
                match p.ptype {
                    PropertyType::One((_, t)) => println!("      {}: {}", p.name, t),
                    PropertyType::Collection((_, t)) => println!("      {}: {}[]", p.name, t),
                }
            }
        }
        if !t.properties.nav_properties.is_empty() {
            println!("    Nav properties:");
            for p in &t.properties.nav_properties {
                match p {
                    NavProperty::Expandable(NavPropertyExpandable {
                        name,
                        ptype: NavPropertyType::One(t),
                        ..
                    }) => println!("      {}: {}", name, t),
                    NavProperty::Expandable(NavPropertyExpandable {
                        name,
                        ptype: NavPropertyType::Collection(t),
                        ..
                    }) => println!("      {}: {}[]", name, t),
                    NavProperty::Reference { cardinality, .. } => match cardinality {
                        OneOrCollection::One(name) => println!("      {}: ref", name),
                        OneOrCollection::Collection(name) => println!("      {}: ref[]", name),
                    },
                }
            }
        }
    }
    println!();
    println!("Entity types:");
    for t in compiled.entity_types.values() {
        print!("  {}", t.name);
        if let Some(base) = t.base {
            print!(" extends {base}");
        }
        println!();
        if !t.properties.is_empty() {
            println!("    properties:");
            for p in &t.properties.properties {
                match p.ptype {
                    PropertyType::One((_, t)) => println!("      {}: {}", p.name, t),
                    PropertyType::Collection((_, t)) => println!("      {}: {}[]", p.name, t),
                }
            }
        }
        if !t.properties.nav_properties.is_empty() {
            println!("    Nav properties:");
            for p in &t.properties.nav_properties {
                match p {
                    NavProperty::Expandable(NavPropertyExpandable {
                        name,
                        ptype: NavPropertyType::One(t),
                        ..
                    }) => println!("      {}: {}", name, t),
                    NavProperty::Expandable(NavPropertyExpandable {
                        name,
                        ptype: NavPropertyType::Collection(t),
                        ..
                    }) => println!("      {}: {}[]", name, t),
                    NavProperty::Reference { cardinality, .. } => match cardinality {
                        OneOrCollection::One(name) => println!("      {}: ref", name),
                        OneOrCollection::Collection(name) => println!("      {}: ref[]", name),
                    },
                }
            }
        }
    }
    println!();
    println!();
    println!("Statistics:");
    println!(" complex types:   {}", compiled.complex_types.len());
    println!(" entity types:    {}", compiled.entity_types.len());
    println!(" enum types:      {}", compiled.enum_types.len());
    println!(" type defs:       {}", compiled.type_definitions.len());
    println!(
        " creatable:       {}",
        compiled.creatable_entity_types.len()
    );
    Ok(())
}
