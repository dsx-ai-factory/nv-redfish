// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Power equipment entities and collections.
//!
//! This module provides typed access to Redfish `PowerEquipment` and the
//! power shelf resources exposed through its `PowerShelves` collection.

use crate::core::NavProperty;
#[cfg(feature = "oem-delta")]
use crate::oem::delta::DeltaPowerShelf;
use crate::schema::power_distribution::PowerDistribution as PowerDistributionSchema;
use crate::schema::power_distribution_collection::PowerDistributionCollection as PowerDistributionCollectionSchema;
use crate::schema::power_equipment::PowerEquipment as PowerEquipmentSchema;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use std::marker::PhantomData;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::power_distribution::PowerEquipmentType;

/// Power equipment service.
///
/// Provides access to root-level power distribution equipment collections.
pub struct PowerEquipment<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<PowerEquipmentSchema>,
}

impl<B: Bmc> PowerEquipment<B> {
    /// Create a new power equipment handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(nav) = &root.root.power_equipment else {
            return Ok(None);
        };

        let data = nav.get(bmc.as_ref()).await.map_err(Error::from)?;

        Ok(Some(Self {
            bmc: bmc.clone(),
            data,
        }))
    }

    /// Get the raw schema data for this power equipment service.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<PowerEquipmentSchema> {
        self.data.clone()
    }

    /// Get the power shelf collection.
    ///
    /// Returns `Ok(None)` when the service does not expose `PowerShelves`.
    ///
    /// # Errors
    ///
    /// Returns an error if retrieving the power shelf collection fails.
    pub async fn power_shelves(&self) -> Result<Option<PowerShelfCollection<B>>, Error<B>> {
        let Some(collection_ref) = &self.data.power_shelves else {
            return Ok(None);
        };

        PowerShelfCollection::new(&self.bmc, collection_ref)
            .await
            .map(Some)
    }
}

/// Power shelf collection.
///
/// Provides functions to access `PowerShelves` members.
pub struct PowerShelfCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<PowerDistributionCollectionSchema>,
}

impl<B: Bmc> PowerShelfCollection<B> {
    async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<PowerDistributionCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = bmc.expand_property(nav).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List all power shelves available in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching power shelf data fails.
    pub async fn members(&self) -> Result<Vec<PowerShelf<B>>, Error<B>> {
        let mut members = Vec::with_capacity(self.collection.members.len());
        for member in &self.collection.members {
            members.push(PowerShelf::new(&self.bmc, member).await?);
        }

        Ok(members)
    }
}

/// Power shelf.
///
/// A power shelf is represented by the Redfish `PowerDistribution` schema with
/// `EquipmentType` set to `PowerShelf`.
pub struct PowerShelf<B: Bmc> {
    #[cfg(feature = "oem-delta")]
    bmc: NvBmc<B>,
    data: Arc<PowerDistributionSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> PowerShelf<B> {
    async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<PowerDistributionSchema>,
    ) -> Result<Self, Error<B>> {
        let data = nav.get(bmc.as_ref()).await.map_err(Error::from)?;
        Ok(Self {
            #[cfg(feature = "oem-delta")]
            bmc: bmc.clone(),
            data,
            _marker: PhantomData,
        })
    }

    /// Get the raw `PowerDistribution` schema data for this power shelf.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<PowerDistributionSchema> {
        self.data.clone()
    }

    /// Delta Energy Systems OEM extension for this power shelf.
    ///
    /// Returns `Ok(None)` when the power shelf does not include Delta OEM
    /// extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing the Delta OEM data fails.
    #[cfg(feature = "oem-delta")]
    pub fn oem_delta(&self) -> Result<Option<DeltaPowerShelf<B>>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map(|oem| DeltaPowerShelf::new(&self.bmc, oem))
            .transpose()
            .map(Option::flatten)
    }
}
