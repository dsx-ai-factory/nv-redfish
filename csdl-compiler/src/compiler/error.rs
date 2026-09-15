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

use crate::compiler::annotations::Error as AnnotationError;
use crate::compiler::QualifiedName;
use crate::edmx::ActionName;
use crate::edmx::Namespace;
use crate::edmx::ParameterName;
use crate::edmx::PropertyName;
use crate::edmx::SimpleIdentifier;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;

/// Compilation error kinds.
#[derive(Debug)]
pub enum Error<'a> {
    /// Error while resolving a protocol annotation.
    Annotation(AnnotationError),
    /// Feature not yet implemented.
    Unimplemented,
    /// Action must be bound.
    NotBoundAction,
    /// Missing binding parameter for an action.
    NoBindingParameterForAction,
    /// Entity type was not found.
    EntityTypeNotFound(QualifiedName<'a>),
    /// Complex type was not found.
    ComplexTypeNotFound(QualifiedName<'a>),
    /// A cycle was found in type inheritance.
    CyclicType(Vec<QualifiedName<'a>>),
    /// A single-valued property closes a complex-type reference cycle,
    /// which generated Rust cannot represent without indirection.
    UnrepresentableCycle(QualifiedName<'a>),
    /// A reference cycle spans more than one complex type — through
    /// properties, a base, or both. Only direct self-reference is
    /// supported: a wider cycle would bake one member's provisional type
    /// info into another's compiled form.
    UnsupportedCycle(QualifiedName<'a>),
    /// Resource.Resource type was not found.
    ResourceTypeNotFound,
    /// Resource.ResourceCollection type was not found.
    ResourceCollectionTypeNotFound,
    /// Error while compiling an entity type.
    EntityType(QualifiedName<'a>, Box<Self>),
    /// Type was not found.
    TypeNotFound(QualifiedName<'a>),
    /// Type definition is not a primitive type.
    TypeDefinitionOfNotPrimitiveType(QualifiedName<'a>),
    /// Error while compiling a type definition.
    TypeDefinition(QualifiedName<'a>, Box<Self>),
    /// Error while compiling a type.
    Type(QualifiedName<'a>, Box<Self>),
    /// Error while compiling a property.
    Property(&'a PropertyName, Box<Self>),
    /// Error while compiling an action.
    Action(&'a ActionName, Box<Self>),
    /// Error while compiling an action return type.
    ActionReturnType(Box<Self>),
    /// Error while compiling an action parameter.
    ActionParameter(&'a ParameterName, Box<Self>),
    /// Error while compiling a singleton.
    Singleton(&'a SimpleIdentifier, Box<Self>),
    /// Error while compiling a schema.
    Schema(&'a Namespace, Box<Self>),
}

impl Display for Error<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Annotation(err) => Display::fmt(err, f),
            Self::Unimplemented => write!(f, "unimplemented"),
            Self::EntityTypeNotFound(v) => write!(f, "entity type not found: {v}"),
            Self::ComplexTypeNotFound(v) => write!(f, "complex type not found: {v}"),
            Self::CyclicType(types) => {
                write!(f, "cyclic type inheritance detected: ")?;
                types.iter().enumerate().try_for_each(|(index, qtype)| {
                    if index > 0 {
                        write!(f, " -> ")?;
                    }
                    write!(f, "{qtype}")
                })
            }
            Self::UnrepresentableCycle(v) => write!(
                f,
                "single-valued property references its own type {v}, which \
                 generated Rust cannot represent without indirection"
            ),
            Self::UnsupportedCycle(v) => write!(
                f,
                "complex-type reference cycle through {v}; only direct \
                 self-reference is supported"
            ),
            Self::ResourceTypeNotFound => write!(f, "Resource.Resource type was not found"),
            Self::ResourceCollectionTypeNotFound => {
                write!(f, "Resource.ResourceCollection type was not found")
            }
            Self::NotBoundAction => {
                write!(f, "unbound action is not supported")
            }
            Self::NoBindingParameterForAction => {
                write!(f, "missing required binding parameter for action")
            }
            Self::EntityType(name, err) => {
                write!(f, "while compiling entity type: {name}\n{err}")
            }
            Self::TypeNotFound(v) => write!(f, "type not found: {v}"),
            Self::TypeDefinitionOfNotPrimitiveType(v) => {
                write!(f, "type definition is not a primitive type: {v}")
            }
            Self::TypeDefinition(name, err) => {
                write!(f, "while compiling type definition: {name}\n{err}")
            }
            Self::Type(name, err) => {
                write!(f, "while compiling type: {name}\n{err}")
            }
            Self::Property(name, err) => {
                write!(f, "while compiling property: {name}\n{err}")
            }
            Self::Action(name, err) => {
                write!(f, "while compiling action: {name}\n{err}")
            }
            Self::ActionReturnType(err) => {
                write!(f, "while compiling action return type\n{err}")
            }
            Self::ActionParameter(name, err) => {
                write!(f, "while compiling action parameter: {name}\n{err}")
            }
            Self::Singleton(name, err) => write!(f, "while compiling singleton: {name}\n{err}"),
            Self::Schema(name, err) => write!(f, "while compiling schema: {name}\n{err}"),
        }
    }
}
