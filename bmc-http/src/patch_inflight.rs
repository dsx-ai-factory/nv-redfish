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

//! In-flight response patching support for [`HttpBmc`](crate::HttpBmc).
//!
//! With the `patch-inflight` feature enabled, each `HttpBmc` owns a registry of
//! JSON patches applied to responses before deserialization. Without the
//! feature, the types here are zero-sized no-ops.

#[cfg(feature = "patch-inflight")]
use std::sync::Arc;

#[cfg(feature = "patch-inflight")]
use nv_redfish_patch_inflight::patch_registry::InflightPatchRegistry;

/// Type to hold in-flight patches registry if "patch-inflight" feature is enabled
#[cfg(feature = "patch-inflight")]
pub type MaybeInflightPatchRegistry = Option<Arc<InflightPatchRegistry>>;

/// Type to stub in-flight patches registry if "patch-inflight" feature is disabled
#[cfg(not(feature = "patch-inflight"))]
pub type MaybeInflightPatchRegistry = Option<()>;

#[cfg(feature = "patch-inflight")]
pub(crate) mod holder {
    use std::sync::{Arc, PoisonError, RwLock};

    use nv_redfish_patch_inflight::patch_registry::InflightPatchRegistry;

    use crate::{CacheableError, HttpBmc, HttpClient};

    use super::MaybeInflightPatchRegistry;

    pub struct MaybeInflightPatchRegistryHolder {
        registry: RwLock<MaybeInflightPatchRegistry>,
    }

    impl MaybeInflightPatchRegistryHolder {
        pub const fn new(registry: MaybeInflightPatchRegistry) -> Self {
            Self {
                registry: RwLock::new(registry),
            }
        }
        pub fn get(&self) -> MaybeInflightPatchRegistry {
            self.registry
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .clone()
        }

        pub fn delete(&self) {
            let mut lock = self
                .registry
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            *lock = None;
        }

        pub fn replace(&self, registry: Arc<InflightPatchRegistry>) {
            let mut lock = self
                .registry
                .write()
                .unwrap_or_else(PoisonError::into_inner);
            *lock = Some(registry);
        }
    }

    impl Default for MaybeInflightPatchRegistryHolder {
        fn default() -> Self {
            Self::new(Some(Arc::new(InflightPatchRegistry::default())))
        }
    }

    impl<C: HttpClient> HttpBmc<C>
    where
        C::Error: CacheableError,
    {
        ///Delete current InflightPatchRegistry to stop patching
        pub fn delete_inflight_patch_registry(&self) {
            self.patch_registry.delete();
        }

        ///Replace current InflightPatchRegistry with a new one
        pub fn replace_inflight_patch_registry(&self, registry: Arc<InflightPatchRegistry>) {
            self.patch_registry.replace(registry);
        }
    }
}

#[cfg(not(feature = "patch-inflight"))]
pub(crate) mod holder {

    use super::MaybeInflightPatchRegistry;

    pub struct MaybeInflightPatchRegistryHolder {
        registry: MaybeInflightPatchRegistry,
    }

    impl MaybeInflightPatchRegistryHolder {
        pub const fn new(registry: MaybeInflightPatchRegistry) -> Self {
            Self { registry }
        }
        pub const fn get(&self) -> MaybeInflightPatchRegistry {
            self.registry
        }
    }

    impl Default for MaybeInflightPatchRegistryHolder {
        fn default() -> Self {
            Self::new(MaybeInflightPatchRegistry::default())
        }
    }
}
