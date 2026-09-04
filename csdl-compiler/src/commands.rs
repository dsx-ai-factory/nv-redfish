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

//! Command-line entry points for the compiler
//!
//! Provides two subcommands used by build scripts or users:
//! - `Compile`: parse and compile one or more CSDL/EDMX files starting
//!   from a root singleton, then generate Rust to an output file.
//! - `CompileOem`: compile OEM schemas into the root set (all types in
//!   the OEM input) while resolving references from additional files.
//!
//! Both commands:
//! - Read EDMX, build a `SchemaBundle`, and compile with optional
//!   `EntityTypeFilter` patterns to limit navigation targets.
//! - Optimize the compiled set and run the Rust generator.
//! - Pretty-print the resulting syntax and write it to the `output` path.

use crate::compiler::ActionFilter;
use crate::compiler::ActionFilterPattern;
use crate::compiler::Config as CompilerConfig;
use crate::compiler::EntityTypeFilter;
use crate::compiler::EntityTypeFilterPattern;
use crate::compiler::PropertyFilter;
use crate::compiler::PropertyPattern;
use crate::compiler::SchemaBundle;
use crate::edmx::Edmx;
use crate::generator::rust::Config as GeneratorConfig;
use crate::generator::rust::RustGenerator;
use crate::optimizer::optimize;
use crate::optimizer::Config as OptimizerConfig;
use crate::Error;
use clap::Subcommand;
use std::collections::BTreeMap;
use std::fs::write;
use std::fs::File;
use std::io::Read as _;
use std::path::PathBuf;

/// Default root singleton to compile.
pub const DEFAULT_ROOT: &str = "Service";

/// Compiler high-level commands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Compile CSDL schemas.
    Compile {
        /// Root service to compile (one of the root singletons in
        /// the EDM document).
        #[arg(short, long, default_value = DEFAULT_ROOT)]
        root: String,
        /// Patterns of entity types to be included to root set even
        /// if they are not referenced from root. If empty, none
        /// additional types are compiled.
        ///
        /// Pattern is a wildcard over the qualified name.
        /// Examples:
        /// `ServiceRoot.*.*` - any entity type in any version of the service root
        /// `SomeNamespace.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any versions of namespace `SomeNamespace`.
        /// `*.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any versions of any namespaces.
        #[arg(short = 'i', long = "include-root-pattern")]
        include_root_patterns: Vec<EntityTypeFilterPattern>,
        /// CSDL documents to compile. In most cases you should
        /// specify all schemas from the Redfish and Swordfish bundles.
        #[arg(required = true)]
        csdls: Vec<String>,
        /// Output file for generated code.
        #[arg(short, long, default_value = "redfish.rs")]
        output: PathBuf,
        /// Patterns of entity types to compile when referenced via a
        /// navigation property. If empty, all entity types are compiled.
        ///
        /// Pattern is a wildcard over the qualified name.
        /// Examples:
        /// `ServiceRoot.*.*` - any entity type in any version of the service root
        /// `SomeNamespace.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any versions of namespace `SomeNamespace`.
        /// `*.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any versions of any namespaces.
        #[arg(short = 'p', long = "pattern")]
        entity_type_patterns: Vec<EntityTypeFilterPattern>,
        /// Patterns of properties that must be compiled with rigid array support
        ///
        /// Pattern is a wildcard over the qualified name.
        /// Examples:
        /// `EthernetInterface.*.EthernetInterface/StaticNameServers` - matches `StaticNameServers` property of `EthernetInterface`
        #[arg(short = 'a', long = "rigid-arrays")]
        rigid_array_patterns: Vec<PropertyPattern>,
    },
    /// Compile OEM CSDL schemas.
    CompileOem {
        /// CSDL documents to compile and include in the root set
        /// (all data types from the OEM schema are compiled).
        #[arg(required = true, value_terminator = "@")]
        root_csdls: Vec<String>,
        /// CSDL documents used for type resolution in `root_csdls`.
        #[arg(index = 2)]
        resolve_csdls: Vec<String>,
        /// Output file for generated code.
        #[arg(short, long, default_value = "redfish.rs")]
        output: PathBuf,
        /// Patterns of entity types to compile when referenced via a
        /// navigation property. If empty, all entity types are compiled.
        ///
        /// Pattern is a wildcard over the qualified name.
        /// Examples:
        /// `ServiceRoot.*.*` - any entity type in any version of the service root
        /// `SomeNamespace.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any versions of namespace `SomeNamespace`.
        /// `*.*.Entity1|Entity2` - `EntityType1` or `EntityType2` from any versions of any namespaces.
        #[arg(short = 'p', long = "pattern")]
        entity_type_patterns: Vec<EntityTypeFilterPattern>,
        /// Patterns of actions to include in the OEM root set.
        ///
        /// Pattern is a wildcard over the action's defining namespace and name.
        /// Example: `NvidiaChassis.*` selects every action in the
        /// `NvidiaChassis` namespace. If empty, no actions are compiled.
        #[arg(long = "action-pattern")]
        action_patterns: Vec<ActionFilterPattern>,
        /// Patterns of properties that must be compiled with rigid array support
        ///
        /// Pattern is a wildcard over the qualified name.
        /// Examples:
        /// `EthernetInterface.*.EthernetInterface/StaticNameServers` - matches `StaticNameServers` property of `EthernetInterface`
        #[arg(short = 'a', long = "rigid-arrays")]
        rigid_array_patterns: Vec<PropertyPattern>,
    },
}

/// Process a compiler command.
///
/// # Errors
///
/// Returns an error if command processing fails.
pub fn process_command(command: &Commands) -> Result<Vec<String>, Error> {
    process_command_inner(command, false)
}

/// Process a compiler command with serialization enabled for read and excerpt models.
///
/// # Errors
///
/// Returns an error if command processing fails.
pub fn process_command_with_read_model_serialization(
    command: &Commands,
) -> Result<Vec<String>, Error> {
    process_command_inner(command, true)
}

fn process_command_inner(
    command: &Commands,
    serialize_read_models: bool,
) -> Result<Vec<String>, Error> {
    let mut display_output = Vec::new();
    match command {
        Commands::Compile {
            root,
            include_root_patterns,
            csdls,
            output,
            entity_type_patterns,
            rigid_array_patterns,
        } => {
            let root_service = root.parse().map_err(Error::WrongRootService)?;
            if csdls.is_empty() {
                return Err(Error::AtLeastOneCSDLFileNeeded);
            }

            let schema_bundle = read_csdls(csdls, &[])?;

            let compiled = schema_bundle
                .compile(
                    &[root_service],
                    &EntityTypeFilter::new_restrictive(include_root_patterns.clone()),
                    CompilerConfig {
                        entity_type_filter: EntityTypeFilter::new_permissive(
                            entity_type_patterns.clone(),
                        ),
                        rigid_array_filter: PropertyFilter::new(rigid_array_patterns.clone()),
                        ..CompilerConfig::default()
                    },
                )
                .map_err(Error::compile_error)?;
            let compiled = optimize(compiled, &OptimizerConfig::default());
            let generator = RustGenerator::new(
                compiled,
                GeneratorConfig {
                    serialize_read_models,
                    ..GeneratorConfig::default()
                },
            )
            .map_err(Error::generate_error)?;

            let syntax_tree =
                syn::parse2::<syn::File>(generator.generate()).map_err(Error::ParseGenerated)?;
            write(output, prettyplease::unparse(&syntax_tree))
                .map_err(|e| Error::WriteOutput(output.clone(), e))?;
            display_output.push(format!("{} file has been written", output.display()));
            Ok(display_output)
        }
        Commands::CompileOem {
            root_csdls,
            resolve_csdls,
            output,
            entity_type_patterns,
            action_patterns,
            rigid_array_patterns,
        } => {
            if root_csdls.is_empty() {
                return Err(Error::AtLeastOneCSDLFileNeeded);
            }
            let schema_bundle = read_csdls(root_csdls, resolve_csdls)?;
            let compiled = schema_bundle
                .compile_all(CompilerConfig {
                    entity_type_filter: EntityTypeFilter::new_permissive(
                        entity_type_patterns.clone(),
                    ),
                    action_filter: ActionFilter::new_restrictive(action_patterns.clone()),
                    rigid_array_filter: PropertyFilter::new(rigid_array_patterns.clone()),
                })
                .map_err(Error::compile_error)?;
            let compiled = optimize(compiled, &OptimizerConfig::default());
            let generator = RustGenerator::new(
                compiled,
                GeneratorConfig {
                    serialize_read_models,
                    ..GeneratorConfig::default()
                },
            )
            .map_err(Error::generate_error)?;
            let syntax_tree =
                syn::parse2::<syn::File>(generator.generate()).map_err(Error::ParseGenerated)?;
            write(output, prettyplease::unparse(&syntax_tree))
                .map_err(|e| Error::WriteOutput(output.clone(), e))?;
            display_output.push(format!("{} file has been written", output.display()));
            Ok(display_output)
        }
    }
}

fn read_csdls(root_csdls: &[String], resolve_csdls: &[String]) -> Result<SchemaBundle, Error> {
    let parse_all = |fnames: &[String]| {
        fnames
            .iter()
            .map(|fname| {
                let mut file = File::open(fname).map_err(|err| Error::Io(fname.clone(), err))?;
                let mut content = String::new();
                file.read_to_string(&mut content)
                    .map_err(|err| Error::Io(fname.clone(), err))?;

                Edmx::parse(&content).map_err(|e| Error::Edmx(fname.clone(), e))
            })
            .collect::<Result<Vec<_>, _>>()
    };

    let root_docs = parse_all(root_csdls)?;
    let resolve_docs = parse_all(resolve_csdls)?;

    root_csdls
        .iter()
        .zip(&root_docs)
        .chain(resolve_csdls.iter().zip(&resolve_docs))
        .flat_map(|(fname, edmx)| {
            edmx.data_services
                .schemas
                .iter()
                .map(move |schema| (schema.namespace.to_string(), fname))
        })
        .fold(
            BTreeMap::<String, Vec<String>>::new(),
            |mut map, (namespace, fname)| {
                map.entry(namespace).or_default().push(fname.clone());
                map
            },
        )
        .into_iter()
        .find_map(|(namespace, files)| (files.len() > 1).then_some((namespace, files)))
        .map_or(Ok(()), |(namespace, files)| {
            Err(Error::DuplicateNamespace(namespace, files))
        })?;

    Ok(SchemaBundle::new(root_docs, resolve_docs))
}
