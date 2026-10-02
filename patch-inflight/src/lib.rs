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
use std::fmt::Display;
use std::sync::Arc;

use crate::patch_registry::InflightPatchRegistry;

/// Errors of patch inflight crate
#[derive(Debug)]
pub enum InflightPatchError {
    /// Duplicated patch name
    DuplicationError(String),
}
impl Display for InflightPatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InflightPatchError::DuplicationError(p) => {
                write!(f, "Duplicated in-flight patch with name {p}")
            }
        }
    }
}
impl std::error::Error for InflightPatchError {}

thread_local! {
    pub static INFLIGHT_PATCH_REGISTRY: RefCell<Option<Arc<InflightPatchRegistry>>> = const { RefCell::new(None) };
}
