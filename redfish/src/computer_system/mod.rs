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
#[cfg(feature = "secure-boot")]
pub mod secure_boot_database;
#[cfg(feature = "storages")]
pub mod storage;

use crate::entity_link::EntityLink;
use crate::patch_support::CollectionWithPatch;
use crate::patch_support::FilterFn;
use crate::patch_support::JsonValue;
use crate::resource::Completeness;
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
pub use item::{BootOptionReference, BootUpdate, ComputerSystem, ComputerSystemUpdate};

#[doc(inline)]
#[cfg(feature = "bios")]
pub use bios::{AttributesUpdate, Bios, BiosUpdate};
#[doc(inline)]
#[cfg(feature = "boot-options")]
pub use boot_option::{BootOption, BootOptionCollection, BootOptionUpdate};
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
#[cfg(feature = "secure-boot")]
pub use secure_boot::SecureBootResetKeysType;
#[doc(inline)]
#[cfg(feature = "secure-boot")]
pub use secure_boot::SecureBootUpdate;
#[doc(inline)]
#[cfg(feature = "secure-boot")]
pub use secure_boot_database::{
    SecureBootDatabase, SecureBootDatabaseCollection, SecureBootDatabaseResetKeysType,
};
#[doc(inline)]
#[cfg(feature = "storages")]
pub use storage::{Storage, StorageController, Volume, VolumeCollection};

/// Link for accessing a computer system.
pub type ComputerSystemLink<B> = EntityLink<B, ComputerSystemSchema>;

/// Computer system collection.
///
/// Provides functions to access collection members.
pub struct SystemCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<ComputerSystemCollectionSchema>,
    completeness: Completeness,
}

impl<B: Bmc> SystemCollection<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let mut filters = Vec::new();
        if let Some(odata_id_filter) = bmc.quirks.filter_computer_system_odata_ids() {
            filters.push(Box::new(move |js: &JsonValue| {
                js.get("@odata.id")
                    .and_then(|v| v.as_str())
                    .map(odata_id_filter)
                    .is_some_and(identity)
            }));
        }
        let filters_fn = (!filters.is_empty())
            .then(move || Arc::new(move |v: &JsonValue| filters.iter().any(|f| f(v))) as FilterFn);

        let id = match &root.root.systems {
            Some(collection_ref) => collection_ref.id().clone(),
            None if bmc.quirks.bug_missing_root_nav_properties() => {
                format!("{}/Systems", root.root.odata_id).into()
            }
            None => return Ok(None),
        };
        let (collection, completeness) = Self::read_collection(
            bmc,
            &NavProperty::new_reference(id),
            None,
            filters_fn.as_ref(),
        )
        .await?;
        Ok(Some(Self {
            bmc: bmc.clone(),
            collection,
            completeness,
        }))
    }

    /// Links to the computer systems in this collection.
    ///
    /// Makes no request. [`EntityLink::upgrade`] loads one
    /// [`ComputerSystem`] without the other members.
    #[must_use]
    pub fn member_links(&self) -> Vec<ComputerSystemLink<B>> {
        self.collection
            .members
            .iter()
            .map(|member| {
                ComputerSystemLink::new(&self.bmc, NavProperty::new_reference(member.id().clone()))
            })
            .collect()
    }

    /// Whether the read of this collection holds all of its members.
    ///
    /// Makes no request.
    #[must_use]
    pub const fn completeness(&self) -> Completeness {
        self.completeness
    }

    /// List all computer systems available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching system data fails.
    pub async fn members(&self) -> Result<Vec<ComputerSystem<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(ComputerSystem::new(&self.bmc, m).await?);
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
        ComputerSystemCollectionSchema {
            odata_id: base.odata_id,
            odata_etag: base.odata_etag,
            odata_type: base.odata_type,
            settings_annotations: base.settings_annotations,
            description: base.description,
            name: base.name,
            oem: base.oem,
            members,
        }
    }
}
