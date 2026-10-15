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

pub(crate) mod fixes;
pub mod patch_registry;
use std::cell::RefCell;
use std::error::Error;
use std::fmt::Display;
use std::sync::Arc;

use serde_json::Value;

use crate::patch_registry::InflightPatchRegistry;

#[derive(Debug)]
pub struct PatchError {
    pub patch_name: String,
    pub error: Box<dyn Error + Sync + Send>,
}

impl Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} : {}", self.patch_name, self.error)
    }
}

/// Errors of patch in-flight crate
#[derive(Debug)]
pub enum InflightPatchError {
    /// Duplicated patch name
    DuplicationError(String),
    /// Particular patch error
    ApplyPatchError(PatchError),
    /// Collection of patches error
    ApplyPatchCollectionErrors(Vec<PatchError>),
}
impl Display for InflightPatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InflightPatchError::DuplicationError(p) => {
                write!(f, "Duplicated in-flight patch with name {p}")
            }
            InflightPatchError::ApplyPatchError(p) => {
                write!(f, "Failed to apply patch {p}")
            }
            InflightPatchError::ApplyPatchCollectionErrors(c) => {
                let collection_string = c
                    .iter()
                    .map(|e| e.to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                write!(f, "Failed to apply patches collection {collection_string}")
            }
        }
    }
}
impl std::error::Error for InflightPatchError {}

thread_local! {
    pub static INFLIGHT_PATCH_REGISTRY: RefCell<Option<Arc<InflightPatchRegistry>>> = const { RefCell::new(None) };
}

pub fn patch_inflight(v: Value) -> Result<Value, InflightPatchError> {
    let registry = INFLIGHT_PATCH_REGISTRY.with_borrow(|r| r.clone());

    if let Some(registry) = registry {
        registry.patch_inflight(v)
    } else {
        Ok(v)
    }
}
