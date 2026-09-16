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

use crate::patch_support::CollectionWithPatch;
use crate::patch_support::Payload;
use crate::patch_support::ReadPatchFn;
use crate::schema::resource::ResourceCollection;
use crate::schema::software_inventory::SoftwareInventory as SoftwareInventorySchema;
use crate::schema::software_inventory_collection::SoftwareInventoryCollection as SoftwareInventoryCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EdmDateTimeOffset;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::sync::Arc;
use tagged_types::TaggedType;

/// Version of the software.
pub type Version = TaggedType<String, VersionTag>;
/// Reference to the version of software.
pub type VersionRef<'a> = TaggedType<&'a str, VersionTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, FromStr, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum VersionTag {}

/// Release date of the software.
pub type ReleaseDate = TaggedType<EdmDateTimeOffset, ReleaseDateTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, FromStr, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum ReleaseDateTag {}

/// Represents a software inventory item in the update service.
///
/// Provides access to software version information and metadata.
pub struct SoftwareInventory<B: Bmc> {
    #[allow(dead_code)]
    bmc: NvBmc<B>,
    data: Arc<SoftwareInventorySchema>,
}

impl<B: Bmc> SoftwareInventory<B> {
    /// Create a new software inventory handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SoftwareInventorySchema>,
        read_patch_fn: Option<&ReadPatchFn>,
    ) -> Result<Self, Error<B>> {
        if let Some(read_patch_fn) = read_patch_fn {
            Payload::get(bmc.as_ref(), nav, read_patch_fn.as_ref()).await
        } else {
            nav.get(bmc.as_ref()).await.map_err(Error::Bmc)
        }
        .map(|data| Self {
            bmc: bmc.clone(),
            data,
        })
    }

    /// Get the raw schema data for this software inventory item.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<SoftwareInventorySchema> {
        self.data.clone()
    }

    /// Get the version of software inventory item.
    #[must_use]
    pub fn version(&self) -> Option<VersionRef<'_>> {
        self.data
            .version
            .as_ref()
            .and_then(Option::as_deref)
            .map(VersionRef::new)
    }

    /// Get the release date of the software inventory item.
    #[must_use]
    pub fn release_date(&self) -> Option<ReleaseDate> {
        self.data
            .release_date
            .and_then(identity)
            .map(ReleaseDate::new)
    }
}

pub struct SoftwareInventoryCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<SoftwareInventoryCollectionSchema>,
    read_patch_fn: Option<ReadPatchFn>,
}

impl<B: Bmc> CollectionWithPatch<SoftwareInventoryCollectionSchema, SoftwareInventorySchema, B>
    for SoftwareInventoryCollection<B>
{
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<SoftwareInventorySchema>>,
    ) -> SoftwareInventoryCollectionSchema {
        SoftwareInventoryCollectionSchema {
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

impl<B: Bmc> SoftwareInventoryCollection<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        collection_ref: &NavProperty<SoftwareInventoryCollectionSchema>,
        read_patch_fn: Option<ReadPatchFn>,
    ) -> Result<Self, Error<B>> {
        let collection =
            Self::expand_collection(bmc, collection_ref, read_patch_fn.as_ref(), None).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
            read_patch_fn,
        })
    }

    pub(crate) async fn members(&self) -> Result<Vec<SoftwareInventory<B>>, Error<B>> {
        let mut items = Vec::new();
        for nav in &self.collection.members {
            items.push(SoftwareInventory::new(&self.bmc, nav, self.read_patch_fn.as_ref()).await?);
        }
        Ok(items)
    }
}
