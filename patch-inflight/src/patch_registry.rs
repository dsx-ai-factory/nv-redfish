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
use std::error::Error;
use std::sync::Arc;

pub use serde_json::Value;
use wildmatch::WildMatch;

use crate::fixes::fix_ntp_null_elements;
use crate::{InflightPatchError, PatchError};

///Function to transform JSON to correct RedFish object
///right before NavPropery deserialization
pub type InflightPatchFn =
    Arc<dyn Fn(Value) -> Result<Value, Box<dyn Error + Sync + Send>> + Sync + Send>;

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

///Defines how InflightPatchRegistry handle patches error
#[derive(Default)]
pub enum ErrorPolicy {
    ///Return error of first failed patch
    #[default]
    FailFast,
    ///Ignore failed patches, apply non-failed (no error emits)
    SkipFailed,
    ///Return original value if any patch fails
    SkipAll,
    ///Fails if at least one patch fails
    ///error contains all failed patches
    CollectFailed,
}

pub struct InflightPatchRegistry {
    patches: Vec<InflightPatch>,
    error_policy: ErrorPolicy,
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

        match InflightPatchRegistry::new(patches, ErrorPolicy::default()) {
            Ok(r) => r,
            Err(_) => Self {
                patches: vec![],
                error_policy: ErrorPolicy::default(),
            },
        }
    }
}

impl InflightPatchRegistry {
    pub fn new(
        mut patches: Vec<InflightPatch>,
        error_policy: ErrorPolicy,
    ) -> Result<Self, InflightPatchError> {
        let mut names = HashSet::with_capacity(patches.len());
        for patch in &patches {
            if !names.insert(patch.name.as_str()) {
                return Err(InflightPatchError::DuplicationError(patch.name.clone()));
            }
        }
        patches.sort_by_key(|e| e.priority);
        Ok(Self {
            patches,
            error_policy,
        })
    }

    //TODO: Cover with test to see how ErrorPolicy work
    fn patch(&self, oid: &str, json: Value) -> Result<Value, InflightPatchError> {
        let matching_patches: Vec<&InflightPatch> = self
            .patches
            .iter()
            .filter(|p| p.oid_predicate.matches(oid))
            .collect();

        match self.error_policy {
            ErrorPolicy::FailFast => matching_patches.into_iter().try_fold(json, |value, patch| {
                (patch.patch)(value).map_err(|e| {
                    InflightPatchError::ApplyPatchError(PatchError {
                        patch_name: patch.name.clone(),
                        error: e,
                    })
                })
            }),
            ErrorPolicy::SkipFailed => {
                Ok(matching_patches.into_iter().fold(json, |value, patch| {
                    (patch.patch)(value.clone()).unwrap_or(value)
                }))
            }
            ErrorPolicy::SkipAll => {
                let origin_json = json.clone();
                let result = matching_patches
                    .into_iter()
                    .try_fold(json, |value, patch| (patch.patch)(value));
                result.or_else(|_| Ok(origin_json))
            }
            ErrorPolicy::CollectFailed => {
                let mut errors = vec![];
                let result = matching_patches
                    .into_iter()
                    .fold(json, |value, patch| match (patch.patch)(value.clone()) {
                        Ok(result_value) => result_value,
                        Err(e) => {
                            let error = PatchError {
                                patch_name: patch.name.clone(),
                                error: e,
                            };
                            errors.push(error);
                            value
                        }
                    });
                if errors.is_empty() {
                    Ok(result)
                } else {
                    Err(InflightPatchError::ApplyPatchCollectionErrors(errors))
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.patches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.patches.is_empty()
    }

    pub fn patch_inflight(&self, mut v: Value) -> Result<Value, InflightPatchError> {
        let oid = v
            .as_object()
            .and_then(|o| o.get("@odata.id"))
            .and_then(|s| s.as_str())
            .map(str::to_owned);

        if let Some(oid) = oid {
            v = self.patch(&oid, v)?;
        }
        Ok(v)
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
