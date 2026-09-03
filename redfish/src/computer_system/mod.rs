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
#[cfg(feature = "memory")]
pub mod memory_metrics;
#[cfg(feature = "processors")]
pub mod processor;
#[cfg(feature = "processors")]
pub mod processor_metrics;
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
#[cfg(feature = "memory")]
pub use memory_metrics::MemoryMetrics;
#[doc(inline)]
#[cfg(feature = "processors")]
pub use processor::Processor;
#[doc(inline)]
#[cfg(feature = "processors")]
pub use processor_metrics::ProcessorMetrics;
#[doc(inline)]
#[cfg(feature = "secure-boot")]
pub use secure_boot::SecureBoot;
#[doc(inline)]
#[cfg(feature = "secure-boot")]
pub use secure_boot::SecureBootCurrentBootType;
#[doc(inline)]
#[cfg(feature = "storages")]
pub use storage::Storage;

/// Computer system collection.
///
/// Provides functions to access collection members.
pub struct SystemCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<ComputerSystemCollectionSchema>,
    read_patch_fn: Option<ReadPatchFn>,
    #[cfg(all(feature = "chassis", feature = "oem-nvidia"))]
    chassis_collection_id: Option<nv_redfish_core::ODataId>,
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
        if bmc.quirks.vera_rubin_composite_boot_order_entries() {
            patches.push(normalize_vera_rubin_composite_boot_order);
        }
        let read_patch_fn = (!patches.is_empty())
            .then(|| Arc::new(move |v| patches.iter().fold(v, |acc, f| f(acc))) as ReadPatchFn);
        let filters_fn = (!filters.is_empty())
            .then(move || Arc::new(move |v: &JsonValue| filters.iter().any(|f| f(v))) as FilterFn);
        #[cfg(all(feature = "chassis", feature = "oem-nvidia"))]
        let chassis_collection_id = root.root.chassis.as_ref().map(|nav| nav.id().clone());

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
                #[cfg(all(feature = "chassis", feature = "oem-nvidia"))]
                chassis_collection_id,
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
            members.push(
                ComputerSystem::new(
                    &self.bmc,
                    m,
                    self.read_patch_fn.as_ref(),
                    #[cfg(all(feature = "chassis", feature = "oem-nvidia"))]
                    self.chassis_collection_id.clone(),
                )
                .await?,
            );
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

/// Vera Rubin firmware reports composite `BootOrder` entries such as
/// `"Boot0019: Ubuntu"` while boot option resources use the bare reference.
fn normalize_vera_rubin_composite_boot_order(mut v: JsonValue) -> JsonValue {
    if let JsonValue::Object(ref mut obj) = v {
        if let Some(JsonValue::Object(ref mut boot)) = obj.get_mut("Boot") {
            if let Some(JsonValue::Array(ref mut boot_order)) = boot.get_mut("BootOrder") {
                for entry in boot_order.iter_mut() {
                    if let JsonValue::String(entry) = entry {
                        *entry = vera_rubin_boot_order_entry_reference(entry).to_string();
                    }
                }
            }
        }
    }
    v
}

fn vera_rubin_boot_order_entry_reference(entry: &str) -> &str {
    entry
        .split_once(": ")
        .map_or(entry, |(reference, _)| reference)
}

#[cfg(test)]
mod vera_rubin_boot_order_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn vera_rubin_boot_order_entry_reference_strips_display_name_suffix() {
        assert_eq!(
            vera_rubin_boot_order_entry_reference("Boot0019: Ubuntu"),
            "Boot0019"
        );
        assert_eq!(
            vera_rubin_boot_order_entry_reference("Boot0010: UEFI HTTPv4 (MAC:AA)"),
            "Boot0010"
        );
        assert_eq!(
            vera_rubin_boot_order_entry_reference("Boot0010"),
            "Boot0010"
        );
    }

    #[test]
    fn normalize_vera_rubin_composite_boot_order_patches_boot_order_array() {
        let patched = normalize_vera_rubin_composite_boot_order(json!({
            "Boot": {
                "BootOrder": [
                    "Boot0019: Ubuntu",
                    "Boot0010: UEFI HTTPv4 (MAC:AA)"
                ]
            }
        }));
        assert_eq!(
            patched,
            json!({
                "Boot": {
                    "BootOrder": ["Boot0019", "Boot0010"]
                }
            })
        );
    }
}
