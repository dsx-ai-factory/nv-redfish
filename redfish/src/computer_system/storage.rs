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

//! Storage subsystem and its respective properties.

use crate::computer_system::Drive;
use crate::patch_support::CollectionWithPatch;
use crate::schema::resource::ResourceCollection;
use crate::schema::storage::Storage as StorageSchema;
use crate::schema::storage_collection::StorageCollection as StorageCollectionSchema;
use crate::schema::volume::Volume as VolumeSchema;
use crate::schema::volume::VolumeCreate;
use crate::schema::volume_collection::VolumeCollection as VolumeCollectionSchema;
use crate::Error;
use crate::NvBmc;
use crate::Resource;
use crate::ResourceSchema;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "oem-dell")]
use crate::oem::dell::DellStorageActions;

/// Represents a storage controller in a computer system.
///
/// Provides access to storage controller information and associated drives.
pub struct Storage<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<StorageSchema>,
}

impl<B: Bmc> Storage<B> {
    /// Create a new storage handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<StorageSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this storage controller.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<StorageSchema> {
        self.data.clone()
    }

    /// Get drives associated with this storage controller.
    ///
    /// Fetches the drive collection and returns a list of [`Drive`] handles.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The storage controller does not have drives
    /// - Fetching drive data fails
    pub async fn drives(&self) -> Result<Option<Vec<Drive<B>>>, Error<B>> {
        if let Some(drives_ref) = &self.data.drives {
            let mut drives = Vec::new();
            for d in drives_ref {
                drives.push(Drive::new(&self.bmc, d).await?);
            }

            Ok(Some(drives))
        } else {
            Ok(None)
        }
    }

    /// Get the advertised volume collection.
    #[must_use]
    pub fn volumes(&self) -> Option<VolumeCollection<B>> {
        self.data
            .volumes
            .as_ref()
            .map(|volumes| VolumeCollection::new(&self.bmc, volumes))
    }

    /// Get Dell OEM actions advertised by this storage controller.
    ///
    /// Returns `Ok(None)` when the resource has no OEM actions object.
    ///
    /// # Errors
    ///
    /// Returns an error if Dell OEM actions cannot be parsed.
    #[cfg(feature = "oem-dell")]
    pub fn oem_dell_actions(&self) -> Result<Option<DellStorageActions<B>>, Error<B>> {
        self.data
            .actions
            .as_ref()
            .and_then(|actions| actions.oem.as_ref())
            .map(|actions| DellStorageActions::new(&self.bmc, actions))
            .transpose()
    }
}

impl<B: Bmc> Resource for Storage<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}

/// A standard Redfish Volume resource.
pub struct Volume<B: Bmc> {
    data: Arc<VolumeSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> Volume<B> {
    /// Wrap generated Volume data.
    pub(crate) const fn from_data(data: Arc<VolumeSchema>) -> Self {
        Self {
            data,
            _marker: PhantomData,
        }
    }

    /// Get the raw Volume schema.
    #[must_use]
    pub fn raw(&self) -> Arc<VolumeSchema> {
        self.data.clone()
    }
}

impl<B: Bmc> Resource for Volume<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}

/// A standard Redfish Volume collection.
pub struct VolumeCollection<B: Bmc> {
    bmc: NvBmc<B>,
    id: ODataId,
}

impl<B: Bmc> VolumeCollection<B> {
    /// Create a collection handle from an advertised navigation property.
    fn new(bmc: &NvBmc<B>, collection: &NavProperty<VolumeCollectionSchema>) -> Self {
        Self {
            bmc: bmc.clone(),
            id: collection.id().clone(),
        }
    }

    /// Get the advertised collection identifier.
    #[must_use]
    pub const fn odata_id(&self) -> &ODataId {
        &self.id
    }

    /// Fetch every Volume in the collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection or a member cannot be fetched.
    pub async fn members(&self) -> Result<Vec<Volume<B>>, Error<B>> {
        let collection = NavProperty::<VolumeCollectionSchema>::new_reference(self.id.clone())
            .get(self.bmc.as_ref())
            .await
            .map_err(Error::Bmc)?;
        let mut volumes = Vec::with_capacity(collection.members.len());
        for member in &collection.members {
            let data = member.get(self.bmc.as_ref()).await.map_err(Error::Bmc)?;
            volumes.push(Volume::from_data(data));
        }
        Ok(volumes)
    }

    /// Create a standard Redfish Volume.
    ///
    /// Embedded entities are used directly.  A Location-only response is
    /// resolved with a follow-up GET.
    ///
    /// # Errors
    ///
    /// Returns an error if the BMC rejects the request or a returned reference
    /// cannot be resolved.
    pub async fn create(
        &self,
        request: &VolumeCreate,
    ) -> Result<ModificationResponse<Volume<B>>, Error<B>> {
        self.bmc
            .as_ref()
            .create::<_, NavProperty<VolumeSchema>>(&self.id, request)
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move {
                nav.get(self.bmc.as_ref())
                    .await
                    .map(Volume::from_data)
                    .map_err(Error::Bmc)
            })
            .await
    }
}

/// Storage collection.
///
/// Provides functions to access collection members.
#[cfg(feature = "storages")]
pub struct StorageCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<StorageCollectionSchema>,
}

#[cfg(feature = "storages")]
impl<B: Bmc> StorageCollection<B> {
    /// Create a new storage collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<StorageCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = Self::expand_collection(bmc, nav, None, None).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List all storages available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching storage data fails.
    pub async fn members(&self) -> Result<Vec<Storage<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(Storage::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

#[cfg(feature = "storages")]
impl<B: Bmc> CollectionWithPatch<StorageCollectionSchema, StorageSchema, B>
    for StorageCollection<B>
{
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<StorageSchema>>,
    ) -> StorageCollectionSchema {
        StorageCollectionSchema { base, members }
    }
}
