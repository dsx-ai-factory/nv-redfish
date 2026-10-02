// SPDX-FileCopyrightText: Copyright (c) 2025-2026 MIRANTIS, INC. & AFFILIATES. All rights reserved.
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

use std::collections::HashSet;
use std::sync::Arc;

pub use serde_json::Value;
use wildmatch::WildMatch;

use crate::fixes::fix_ntp_null_elements;
use crate::InflightPatchError;

///Function to transform JSON to correct RedFish object
///right before NavPropery deserialization
pub type InflightPatchFn = Arc<dyn Fn(Value) -> Value + Sync + Send>;

pub struct ODataIdMatcher(WildMatch);

impl From<&str> for ODataIdMatcher {
    fn from(s: &str) -> Self {
        Self(WildMatch::new(s))
    }
}

impl ODataIdMatcher {
    pub fn matches(&self, oid: &str) -> bool {
        self.0.matches(oid)
    }
}

pub struct InflightPatch {
    pub priority: usize,
    pub name: String,
    pub oid_predicate: ODataIdMatcher,
    pub patch: InflightPatchFn,
}

pub struct InflightPatchRegistry {
    patches: Vec<InflightPatch>,
}

impl Default for InflightPatchRegistry {
    fn default() -> Self {
        let mut patches = vec![];

        let fix_ntp_null = InflightPatch {
            priority: 1000,
            name: "fix_ntp_null".into(),
            oid_predicate: "/redfish/v1/Managers/*/NetworkProtocol".into(),
            patch: Arc::new(fix_ntp_null_elements),
        };
        patches.push(fix_ntp_null);

        match InflightPatchRegistry::new(patches) {
            Ok(r) => r,
            Err(_) => Self { patches: vec![] },
        }
    }
}

impl InflightPatchRegistry {
    pub fn new(mut patches: Vec<InflightPatch>) -> Result<Self, InflightPatchError> {
        let mut names = HashSet::with_capacity(patches.len());
        for patch in &patches {
            if !names.insert(patch.name.as_str()) {
                return Err(InflightPatchError::DuplicationError(patch.name.clone()));
            }
        }
        patches.sort_by_key(|e| e.priority);
        Ok(Self { patches })
    }

    fn patch(&self, oid: &str, json: Value) -> Value {
        self.patches
            .iter()
            .filter(|p| p.oid_predicate.matches(oid))
            .map(|p| p.patch.clone())
            .fold(json, |i, f| f(i))
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn patch_inflight(&self, mut v: Value) -> Value {
        let oid = v
            .as_object()
            .and_then(|o| o.get("@odata.id"))
            .and_then(|s| s.as_str())
            .map(str::to_owned);

        if let Some(oid) = oid {
            v = self.patch(&oid, v);
        }
        v
    }
}

#[cfg(test)]
mod test {
    use crate::patch_registry::InflightPatchRegistry;

    #[test]
    fn test_default_registry_contains_elements() {
        assert!(InflightPatchRegistry::default().len() > 0);
    }
}
