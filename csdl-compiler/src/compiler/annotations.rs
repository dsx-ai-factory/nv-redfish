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

//! Shared compiled protocol annotations.

use crate::compiler::{
    ensure_type, Compiled, Context, Error as CompilerError, MapType, MustHaveId, OData,
    QualifiedName, Stack, TypeClass,
};
use crate::edmx::{Namespace, SimpleIdentifier, Term};
use crate::OneOrCollection;
use std::fmt::{Display, Formatter, Result as FmtResult};

/// Errors while resolving protocol annotations.
#[derive(Debug)]
pub enum Error {
    /// The named annotation's vocabulary term or resource type was not found.
    NotFound(&'static str),
    /// The term must reference a single value of the expected type class.
    InvalidType(&'static str, TypeClass),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::NotFound(name) => write!(f, "annotation not found: {name}"),
            Self::InvalidType(name, expected) => {
                write!(f, "annotation {name} must reference a single {expected:?}")
            }
        }
    }
}

/// Protocol annotations shared by all types in a compilation.
#[derive(Debug, Default)]
pub struct Annotations<'a> {
    /// Resolved `@Redfish.Settings` resource annotation.
    pub redfish_settings: Option<Annotation<'a>>,
    /// Resolved `@Redfish.SettingsApplyTime` annotation.
    pub redfish_settings_apply_time: Option<Annotation<'a>>,
}

impl Annotations<'_> {
    /// Merge annotations, preferring values present in `other`.
    #[must_use]
    // Consume both fragments like Compiled::merge, allowing future owned fields.
    #[allow(clippy::needless_pass_by_value)]
    pub fn merge(self, other: Self) -> Self {
        // Keep both the pattern and initializer exhaustive so new annotations
        // require an explicit merge rule.
        let Self {
            redfish_settings,
            redfish_settings_apply_time,
        } = other;
        Self {
            redfish_settings: redfish_settings.or(self.redfish_settings),
            redfish_settings_apply_time: redfish_settings_apply_time
                .or(self.redfish_settings_apply_time),
        }
    }
}

impl<'a> MapType<'a> for Annotations<'a> {
    fn map_type<F>(self, f: F) -> Self
    where
        F: Fn(QualifiedName<'a>) -> QualifiedName<'a>,
    {
        Self {
            redfish_settings: self.redfish_settings.map(|a| a.map_type(&f)),
            redfish_settings_apply_time: self.redfish_settings_apply_time.map(|a| a.map_type(&f)),
        }
    }
}

/// Compile the annotations already exposed on generated resource structures.
pub(crate) fn compile_for_resources<'a>(
    ctx: &Context<'a>,
    stack: &Stack<'a, '_>,
) -> Result<Compiled<'a>, CompilerError<'a>> {
    let stack = stack.new_frame();
    let (compiled, settings) =
        Annotation::compile("@Redfish.Settings", TypeClass::ComplexType, ctx, &stack)?;
    let stack = stack.merge(compiled);
    let (compiled, apply_time) = Annotation::compile(
        "@Redfish.SettingsApplyTime",
        TypeClass::ComplexType,
        ctx,
        &stack,
    )?;
    let mut compiled = stack.merge(compiled).done();
    compiled.annotations.redfish_settings = Some(settings);
    compiled.annotations.redfish_settings_apply_time = Some(apply_time);
    Ok(compiled)
}

/// Resolved vocabulary term, carried through the normal type pipeline.
#[derive(Debug, Clone, Copy)]
pub struct Annotation<'a> {
    /// The vocabulary term, including its documentation.
    pub term: &'a Term,
    /// The resolved schema type referenced by the term's Type attribute.
    pub value_type: QualifiedName<'a>,
}

impl<'a> Annotation<'a> {
    /// Resolve a Redfish vocabulary term and compile its value type with the schema.
    // Only the fixed vocabulary namespace is parsed with expect.
    #[allow(clippy::unwrap_in_result)]
    fn compile(
        annotation: &'static str,
        expected: TypeClass,
        ctx: &Context<'a>,
        stack: &Stack<'a, '_>,
    ) -> Result<(Compiled<'a>, Self), CompilerError<'a>> {
        let namespace: Namespace = "RedfishExtensions.v1_0_0"
            .parse()
            .expect("valid vocabulary namespace");
        let name: SimpleIdentifier = annotation
            .strip_prefix("@Redfish.")
            .and_then(|name| name.parse().ok())
            .ok_or(Error::NotFound(annotation))
            .map_err(CompilerError::Annotation)?;
        let term = ctx
            .schema_index
            .find_term(QualifiedName::new(&namespace, &name))
            .ok_or(Error::NotFound(annotation))
            .map_err(CompilerError::Annotation)?;
        let Some(OneOrCollection::One(name)) = &term.ttype else {
            return Err(CompilerError::Annotation(Error::InvalidType(
                annotation, expected,
            )));
        };
        // Settings terms refer to abstract types; retain the most specific version.
        let value_type = ctx.schema_index.find_child_type(name.into());
        ctx.schema_index
            .find_type(value_type)
            .ok_or(Error::NotFound(annotation))
            .map_err(CompilerError::Annotation)?;
        let (compiled, info) = ensure_type(value_type, ctx, stack)?;
        if info.class != expected {
            return Err(CompilerError::Annotation(Error::InvalidType(
                annotation, expected,
            )));
        }
        Ok((compiled, Self { term, value_type }))
    }

    /// Documentation and metadata on the vocabulary term.
    #[must_use]
    pub fn odata(&self) -> OData<'a> {
        OData::new(MustHaveId::new(false), self.term)
    }
}

impl<'a> MapType<'a> for Annotation<'a> {
    fn map_type<F>(mut self, f: F) -> Self
    where
        F: Fn(QualifiedName<'a>) -> QualifiedName<'a>,
    {
        self.value_type = f(self.value_type);
        self
    }
}
