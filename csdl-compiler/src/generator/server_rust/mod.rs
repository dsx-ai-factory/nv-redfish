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

//! Serializable server models, independent of the client read-model generator.
//!
//! Explicit entity/complex roots select a schema family. Inheritance is flattened,
//! structural and excerpt dependencies are included. Ordinary navigation properties
//! remain links; excerpt copies use separate models containing the selected fields.
//! Write-only and excerpt-only properties are omitted from full response models.
//! The output is one file with inline namespace modules, suitable for `include!`.
//! Types pass through the same optimizer as client models before emission.
//! All entry points take [`QualifiedName`](crate::compiler::QualifiedName) roots; callers parse names.
//!
//! Follow the shared [generator implementation conventions](super).

mod annotation;
mod config;
mod enum_def;
mod mod_def;
mod property;
mod property_def;
mod struct_def;
mod type_def;
mod types;

#[cfg(test)]
mod tests;

use std::error::Error as StdError;
use std::fmt;
use std::fs::read_to_string;
use std::io::Error as IoError;
use std::path::PathBuf;

use crate::compiler::{
    Config as CompilerConfig, EntityTypeFilter, EntityTypeFilterPattern, ExcerptDependencies,
    QualifiedName, SchemaBundle,
};
use crate::edmx::{Edmx, QualifiedTypeName, ValidateError};
use crate::optimizer::{optimize, Config as OptimizerConfig};

use enum_def::EnumDef;
use mod_def::{Definition, ModDef};
use struct_def::{GenerateType, StructDef};
use type_def::TypeDef;

pub use config::Config;

/// A schema cannot be loaded, compiled, or represented by the response generator.
#[derive(Debug)]
pub enum Error {
    /// An input schema could not be read.
    Read(PathBuf, IoError),
    /// An input schema is invalid.
    Parse(PathBuf, ValidateError),
    /// An explicit root or one of its dependencies failed compilation.
    Compile(String),
    /// A primitive has no supported response representation yet.
    UnsupportedPrimitive(QualifiedTypeName),
    /// Distinct schema identifiers would generate the same Rust identifier.
    NameCollision(syn::Path),
    /// The generated syntax is invalid.
    Syntax(syn::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(path, _) => write!(f, "cannot read {}", path.display()),
            Self::Parse(path, _) => write!(f, "cannot parse {}", path.display()),
            Self::Compile(error) => f.write_str(error),
            Self::UnsupportedPrimitive(name) => write!(f, "unsupported response primitive: {name}"),
            Self::NameCollision(path) => {
                f.write_str("response Rust name collision: ")?;
                path.segments
                    .iter()
                    .enumerate()
                    .try_for_each(|(index, segment)| {
                        if index > 0 {
                            f.write_str("::")?;
                        }
                        fmt::Display::fmt(&segment.ident, f)
                    })
            }
            Self::Syntax(_) => f.write_str("invalid generated response syntax"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Read(_, error) => Some(error),
            Self::Parse(_, error) => Some(error),
            Self::Syntax(error) => Some(error),
            _ => None,
        }
    }
}

/// Schemas used to generate server models.
#[derive(Clone, Copy)]
pub enum Input<'a> {
    /// An already parsed schema bundle.
    Bundle(&'a SchemaBundle),
    /// Local CSDL files to load and parse.
    Files(&'a [PathBuf]),
}

/// Builder for serializable server models with explicit entity or complex roots.
///
/// Constructors require every mandatory property, directly or in a generated
/// `<Type>Required` structure when their number exceeds the configured threshold.
/// Entity constructors initialize `@odata.type` from the resolved CSDL type name.
/// Optional fields start absent. Required nullable values serialize even when null.
#[must_use]
pub struct Builder<'a> {
    roots: &'a [QualifiedName<'a>],
    config: Config,
}

impl<'a> Builder<'a> {
    /// Select the entity or complex type roots to generate.
    pub fn new(roots: &'a [QualifiedName<'a>]) -> Self {
        Self {
            roots,
            config: Config::default(),
        }
    }

    /// Set constructor generation options.
    pub const fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// Generate a single response module from parsed schemas or local CSDL files.
    ///
    /// Performs no network access and does not write files or emit Cargo directives.
    ///
    /// # Errors
    ///
    /// Returns a schema loading, compilation, or generation error.
    pub fn generate(self, input: Input<'_>) -> Result<String, Error> {
        let loaded_bundle;
        let bundle = match input {
            Input::Bundle(bundle) => bundle,
            Input::Files(paths) => {
                let mut paths: Vec<_> = paths.iter().collect();
                paths.sort_unstable();
                paths.dedup();
                let documents = paths
                    .into_iter()
                    .map(|path| {
                        let source =
                            read_to_string(path).map_err(|e| Error::Read(path.clone(), e))?;
                        Edmx::parse(&source).map_err(|e| Error::Parse(path.clone(), e))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                loaded_bundle = SchemaBundle::new(documents, Vec::new());
                &loaded_bundle
            }
        };
        let (compiled, roots) = bundle
            .compile_named_types(
                self.roots,
                CompilerConfig {
                    entity_type_filter: EntityTypeFilter::new_restrictive(Vec::new()),
                    excerpt_dependencies: ExcerptDependencies::Include,
                    ..CompilerConfig::default()
                },
            )
            .map_err(|e| Error::Compile(e.to_string()))?;
        // An explicitly selected entity keeps its wire type even when its
        // fields are identical to a base version. Other types may still fold.
        let optimizer_config = OptimizerConfig {
            never_prune: OptimizerConfig::default().never_prune.including(
                roots
                    .into_iter()
                    .filter(|name| compiled.entity_types.contains_key(name))
                    .map(EntityTypeFilterPattern::from),
            ),
        };
        let compiled = optimize(compiled, &optimizer_config);
        let names = compiled.sorted_type_names();
        let mut root = ModDef::default();
        for name in names {
            let definition = if let Some(value) = compiled.enum_types.get(&name) {
                Definition::Enum(EnumDef::new(value))
            } else if let Some(value) = compiled.type_definitions.get(&name) {
                Definition::Type(TypeDef::new(value))
            } else {
                Definition::Struct(StructDef::new(
                    &compiled,
                    name,
                    self.config,
                    GenerateType::Response,
                ))
            };
            root.add(name, definition);
            for excerpt in compiled.sorted_excerpt_copies(name) {
                root.add(
                    name,
                    Definition::Struct(StructDef::new(
                        &compiled,
                        name,
                        self.config,
                        GenerateType::Excerpt(excerpt),
                    )),
                );
            }
        }
        let syntax = syn::parse2::<syn::File>(root.generate()?).map_err(Error::Syntax)?;
        Ok(format!(
            "// @generated by nv-redfish-csdl-compiler; do not edit.\n{}",
            prettyplease::unparse(&syntax)
        ))
    }
}
