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

use crate::compiler::Error as CompileError;
use crate::edmx::attribute_values::Error as AttributeValuesError;
use crate::edmx::ValidateError;
use crate::generator::client_rust::Error as GenerateError;
use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io::Error as IoError;
use std::path::PathBuf;

/// CSDL Compiler errors.
#[derive(Debug)]
pub enum Error {
    AtLeastOneCSDLFileNeeded,
    Io(String, IoError),
    Edmx(String, ValidateError),
    DuplicateNamespace(String, Vec<String>),
    Compile(Vec<String>),
    WrongRootService(AttributeValuesError),
    Generate(Vec<String>),
    ParseGenerated(syn::Error),
    WriteOutput(PathBuf, IoError),
}

// Passing by reference would break possibility to use it as
// `map_err(Error::compile_error)` etc.
#[allow(clippy::needless_pass_by_value)]
impl Error {
    pub fn compile_error(e: CompileError<'_>) -> Self {
        Self::Compile(
            format!("{e}")
                .split('\n')
                .map(ToString::to_string)
                .collect(),
        )
    }
    pub fn generate_error(e: GenerateError<'_>) -> Self {
        Self::Generate(
            format!("{e}")
                .split('\n')
                .map(ToString::to_string)
                .collect(),
        )
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter) -> FmtResult {
        match self {
            Self::AtLeastOneCSDLFileNeeded => {
                "at least one CSDL file is needed for compilation".fmt(f)
            }
            Self::Io(fname, error) => write!(f, "input/output error: file: {fname}: {error}"),
            Self::Edmx(fname, error) => {
                write!(f, "EDMX format validation error: file: {fname}: {error}")
            }
            Self::DuplicateNamespace(namespace, files) => {
                write!(f, "duplicate CSDL namespace detected: {namespace}")?;
                files.iter().try_for_each(|file| write!(f, "\n - {file}"))
            }
            Self::Compile(lines) => {
                write!(f, "compilation error:")?;
                lines
                    .iter()
                    .enumerate()
                    .try_for_each(|(no, line)| write!(f, "\n #{no}: {line}"))
            }
            Self::WrongRootService(error) => write!(f, "root service format error: {error}"),
            Self::Generate(lines) => {
                write!(f, "generation error:")?;
                lines
                    .iter()
                    .enumerate()
                    .try_for_each(|(no, line)| write!(f, "\n #{no}: {line}"))
            }
            Self::ParseGenerated(error) => {
                write!(f, "failed to parse generated file: {error}")
            }
            Self::WriteOutput(fname, error) => {
                write!(f, "failed write output file: {}: {error}", fname.display())
            }
        }
    }
}

impl StdError for Error {}
