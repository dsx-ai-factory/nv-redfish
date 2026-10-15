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

use crate::edmx::Namespace as EdmxNamespace;
use crate::edmx::SimpleIdentifier;
use std::cmp::Ordering;
use std::fmt::Debug;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::hash::Hash;
use std::hash::Hasher;

/// Wrapper around an EDMX namespace that enables operations like
/// pruning trailing identifiers.
#[derive(Clone, Copy)]
pub struct Namespace<'a> {
    edmx_ns: &'a EdmxNamespace,
    len: usize,
}

#[allow(clippy::len_without_is_empty)] // Namespace cannot be empty.
impl<'a> Namespace<'a> {
    /// Create a new namespace wrapper.
    #[must_use]
    pub const fn new(edmx_ns: &'a EdmxNamespace) -> Self {
        Self {
            edmx_ns,
            len: edmx_ns.ids.len(),
        }
    }

    /// The full CSDL namespace before any truncation by the optimizer.
    #[must_use]
    pub const fn original(&self) -> Self {
        Self::new(self.edmx_ns)
    }

    /// Number of identifiers in the namespace.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Identifier at the specified depth.
    #[must_use]
    pub fn get_id(&self, depth: usize) -> Option<&'a SimpleIdentifier> {
        if self.len > depth {
            Some(&self.edmx_ns.ids[depth])
        } else {
            None
        }
    }

    /// Namespace truncated to its first identifier: the schema family
    /// root, without version segments.
    #[must_use]
    pub const fn root(&self) -> Self {
        Self {
            edmx_ns: self.edmx_ns,
            len: 1,
        }
    }

    /// Namespace truncated to at most `len` identifiers (a no-op when
    /// it is already shorter).
    #[must_use]
    pub const fn truncated(&self, len: usize) -> Self {
        Self {
            edmx_ns: self.edmx_ns,
            len: if len < self.len { len } else { self.len },
        }
    }

    /// Parent namespace (for namespaces with at least two identifiers).
    #[must_use]
    pub const fn parent(&self) -> Option<Self> {
        if self.len > 1 {
            Some(Self {
                edmx_ns: self.edmx_ns,
                len: self.len - 1,
            })
        } else {
            None
        }
    }

    /// Whether this namespace is `Edm`.
    #[must_use]
    pub fn is_edm(&self) -> bool {
        self.len == 1 && self.edmx_ns.is_edm()
    }
}

impl PartialEq for Namespace<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.len == other.len && self.edmx_ns.ids[..self.len] == other.edmx_ns.ids[..self.len]
    }
}

impl Eq for Namespace<'_> {}

impl PartialOrd for Namespace<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Namespace<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.edmx_ns.ids[..self.len].cmp(&other.edmx_ns.ids[..other.len])
    }
}

impl Hash for Namespace<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.edmx_ns.ids[..self.len].hash(state);
    }
}

impl Display for Namespace<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mut iter = self.edmx_ns.ids[..self.len].iter();
        if let Some(v) = iter.next() {
            Display::fmt(&v, f)?;
        }
        for v in iter {
            write!(f, ".{v}")?;
        }
        Ok(())
    }
}

impl Debug for Namespace<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(self, f)
    }
}

#[cfg(test)]
mod tests {
    use super::Namespace;
    use crate::edmx::Namespace as EdmxNamespace;
    use std::str::FromStr as _;

    #[test]
    fn truncated_shortens_and_clamps() {
        let edmx = EdmxNamespace::from_str("NvidiaPortMetrics.v1_6_0").expect("valid namespace");
        let ns = Namespace::new(&edmx);

        assert_eq!(ns.to_string(), "NvidiaPortMetrics.v1_6_0");
        assert_eq!(ns.truncated(1).to_string(), "NvidiaPortMetrics");
        assert_eq!(ns.truncated(2).to_string(), "NvidiaPortMetrics.v1_6_0");
        // Truncating beyond the length is a no-op, not a panic.
        assert_eq!(ns.truncated(9).to_string(), "NvidiaPortMetrics.v1_6_0");
    }
}
