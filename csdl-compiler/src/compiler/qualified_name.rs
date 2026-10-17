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

use crate::compiler::Namespace;
use crate::edmx::Namespace as EdmxNamespace;
use crate::edmx::QualifiedName as EdmxQualifiedName;
use crate::edmx::QualifiedTypeName;
use crate::edmx::SimpleIdentifier;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;

/// Qualified (namespace + identifier) name used during compilation.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy, Debug)]
pub struct QualifiedName<'a> {
    /// Namespace of the type.
    pub namespace: Namespace<'a>,
    /// Unqualified identifier.
    pub name: &'a SimpleIdentifier,
}

impl<'a> QualifiedName<'a> {
    /// Create a new qualified name.
    #[must_use]
    pub const fn new(namespace: &'a EdmxNamespace, name: &'a SimpleIdentifier) -> Self {
        Self {
            namespace: Namespace::new(namespace),
            name,
        }
    }
}

impl<'a> From<&'a QualifiedTypeName> for QualifiedName<'a> {
    fn from(v: &'a QualifiedTypeName) -> Self {
        Self {
            namespace: Namespace::new(&v.inner().namespace),
            name: &v.inner().name,
        }
    }
}

impl From<QualifiedName<'_>> for QualifiedTypeName {
    fn from(value: QualifiedName<'_>) -> Self {
        Self::new(EdmxQualifiedName {
            namespace: EdmxNamespace {
                ids: (0..value.namespace.len())
                    .filter_map(|depth| value.namespace.get_id(depth))
                    .cloned()
                    .collect(),
            },
            name: value.name.clone(),
        })
    }
}

impl Display for QualifiedName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}.{}", self.namespace, self.name)
    }
}
