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
use crate::schema::storage::Storage as StorageSchema;
use crate::schema::storage::StorageController as EmbeddedStorageControllerSchema;
use crate::schema::storage_controller::StorageController as StorageControllerSchema;
use crate::Error;
use crate::NvBmc;
use crate::Resource;
use crate::ResourceSchema;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::marker::PhantomData;
use std::sync::Arc;

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

    /// Get the storage controllers listed in this resource's
    /// `StorageControllers` property.
    ///
    /// Redfish deprecated this property in favor of `Controllers`, but many
    /// BMCs still embed it.  Entries are returned without a request.
    ///
    /// # Errors
    ///
    /// Returns [`Error::StorageControllerNotExpanded`] if an entry is a
    /// reference instead of an embedded object.
    pub async fn storage_controllers(
        &self,
    ) -> Result<Option<Vec<Arc<EmbeddedStorageControllerSchema>>>, Error<B>> {
        if let Some(controllers_ref) = &self.data.storage_controllers {
            let mut controllers = Vec::with_capacity(controllers_ref.len());
            for c in controllers_ref {
                // Entries point into this resource (`...#/StorageControllers/0`),
                // so a reference cannot be fetched on its own.
                if matches!(c, NavProperty::Reference(_)) {
                    return Err(Error::StorageControllerNotExpanded(c.id().clone()));
                }
                controllers.push(c.get(self.bmc.as_ref()).await.map_err(Error::Bmc)?);
            }
            Ok(Some(controllers))
        } else {
            Ok(None)
        }
    }

    /// Get storage controllers from the `Controllers` collection.
    ///
    /// Fetches the collection and returns a list of [`StorageController`]
    /// handles.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the collection or a member fails.
    pub async fn controllers(&self) -> Result<Option<Vec<StorageController<B>>>, Error<B>> {
        if let Some(collection_ref) = &self.data.controllers {
            let collection = collection_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?;
            let mut controllers = Vec::with_capacity(collection.members.len());
            for m in &collection.members {
                let data = m.get(self.bmc.as_ref()).await.map_err(Error::Bmc)?;
                controllers.push(StorageController::from_data(data));
            }
            Ok(Some(controllers))
        } else {
            Ok(None)
        }
    }
}

/// A standard Redfish `StorageController` resource.
pub struct StorageController<B: Bmc> {
    data: Arc<StorageControllerSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> StorageController<B> {
    /// Wrap generated `StorageController` data.
    pub(crate) const fn from_data(data: Arc<StorageControllerSchema>) -> Self {
        Self {
            data,
            _marker: PhantomData,
        }
    }

    /// Get the raw `StorageController` schema.
    #[must_use]
    pub fn raw(&self) -> Arc<StorageControllerSchema> {
        self.data.clone()
    }
}

impl<B: Bmc> Resource for Storage<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}
