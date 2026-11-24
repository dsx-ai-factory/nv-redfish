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

//! Computer System entities and collections.
//!
//! This module provides types for working with Redfish ComputerSystem resources
//! and their sub-resources like processors, storage, memory, and drives.

mod item;

#[cfg(feature = "bios")]
pub mod bios;
#[cfg(feature = "boot-options")]
pub mod boot_option;
#[cfg(feature = "storages")]
pub mod drive;
#[cfg(feature = "memory")]
pub mod memory;
#[cfg(feature = "processors")]
pub mod processor;
#[cfg(feature = "secure-boot")]
pub mod secure_boot;
#[cfg(feature = "storages")]
pub mod storage;

use crate::patch_support::CollectionWithPatch;
use crate::patch_support::FilterFn;
use crate::patch_support::JsonValue;
use crate::patch_support::ReadPatchFn;
use crate::resource::Resource as _;
use crate::schema::computer_system::ComputerSystem as ComputerSystemSchema;
use crate::schema::computer_system_collection::ComputerSystemCollection as ComputerSystemCollectionSchema;
use crate::schema::resource::ResourceCollection;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::sync::Arc;

#[doc(inline)]
pub use item::BootOptionReference;
#[doc(inline)]
pub use item::ComputerSystem;

#[doc(inline)]
#[cfg(feature = "bios")]
pub use bios::Bios;
#[doc(inline)]
#[cfg(feature = "boot-options")]
pub use boot_option::BootOption;
#[doc(inline)]
#[cfg(feature = "boot-options")]
pub use boot_option::BootOptionCollection;
#[doc(inline)]
#[cfg(feature = "storages")]
pub use drive::Drive;
#[doc(inline)]
#[cfg(feature = "memory")]
pub use memory::Memory;
#[doc(inline)]
#[cfg(feature = "processors")]
pub use processor::Processor;
#[doc(inline)]
#[cfg(feature = "secure-boot")]
pub use secure_boot::SecureBoot;
#[doc(inline)]
#[cfg(feature = "secure-boot")]
pub use secure_boot::SecureBootCurrentBootType;
#[doc(inline)]
#[cfg(feature = "storages")]
pub use storage::{Storage, StorageController};

/// Computer system collection.
///
/// Provides functions to access collection members.
pub struct SystemCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<ComputerSystemCollectionSchema>,
    read_patch_fn: Option<ReadPatchFn>,
}

impl<B: Bmc> SystemCollection<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let mut patches = Vec::new();
        let mut filters = Vec::new();
        if let Some(odata_id_filter) = bmc.quirks.filter_computer_system_odata_ids() {
            filters.push(Box::new(move |js: &JsonValue| {
                js.get("@odata.id")
                    .and_then(|v| v.as_str())
                    .map(odata_id_filter)
                    .is_some_and(identity)
            }));
        }
        if bmc.quirks.computer_systems_wrong_last_reset_time() {
            patches.push(computer_systems_wrong_last_reset_time as fn(JsonValue) -> JsonValue);
        }
        if bmc.quirks.bug_empty_uuid_field() {
            patches.push(normalize_empty_uuid_field);
        }
        let read_patch_fn = (!patches.is_empty())
            .then(|| Arc::new(move |v| patches.iter().fold(v, |acc, f| f(acc))) as ReadPatchFn);
        let filters_fn = (!filters.is_empty())
            .then(move || Arc::new(move |v: &JsonValue| filters.iter().any(|f| f(v))) as FilterFn);

        if let Some(collection_ref) = &root.root.systems {
            Self::expand_collection(
                bmc,
                collection_ref,
                read_patch_fn.as_ref(),
                filters_fn.as_ref(),
            )
            .await
            .map(Some)
        } else if bmc.quirks.bug_missing_root_nav_properties() {
            bmc.expand_property(&NavProperty::new_reference(
                format!("{}/Systems", root.odata_id()).into(),
            ))
            .await
            .map(Some)
        } else {
            Ok(None)
        }
        .map(|c| {
            c.map(|collection| Self {
                bmc: bmc.clone(),
                collection,
                read_patch_fn,
            })
        })
    }

    /// List all computer systems available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching system data fails.
    pub async fn members(&self) -> Result<Vec<ComputerSystem<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(ComputerSystem::new(&self.bmc, m, self.read_patch_fn.as_ref()).await?);
        }
        Ok(members)
    }
}

impl<B: Bmc> CollectionWithPatch<ComputerSystemCollectionSchema, ComputerSystemSchema, B>
    for SystemCollection<B>
{
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<ComputerSystemSchema>>,
    ) -> ComputerSystemCollectionSchema {
        ComputerSystemCollectionSchema { base, members }
    }
}

// `LastResetTime` is marked as `edm.DateTimeOffset`, but some systems
// puts "0000-00-00T00:00:00+00:00" as LastResetTime that is not
// conform to ABNF of the DateTimeOffset. We delete such fields...
fn computer_systems_wrong_last_reset_time(v: JsonValue) -> JsonValue {
    if let JsonValue::Object(mut obj) = v {
        if let Some(JsonValue::String(date)) = obj.get("LastResetTime") {
            if date.starts_with("0000-00-00") {
                obj.remove("LastResetTime");
            }
        }
        JsonValue::Object(obj)
    } else {
        v
    }
}

fn normalize_empty_uuid_field(mut v: JsonValue) -> JsonValue {
    if let JsonValue::Object(ref mut obj) = v {
        if let Some(uuid) = obj.get_mut("UUID") {
            let is_empty = uuid.as_str().is_some_and(str::is_empty);
            if is_empty {
                *uuid = JsonValue::Null;
            }
        }
    }
    v
}
