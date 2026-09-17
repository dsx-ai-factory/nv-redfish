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

//! Lenovo persistent boot-order resources.

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::oem::lenovo::schema::lenovo_boot_manager::LenovoBootManager as LenovoBootManagerSchema;
use crate::oem::lenovo::schema::lenovo_boot_manager_collection::LenovoBootManagerCollection as LenovoBootManagerCollectionSchema;
use crate::Error;
use crate::NvBmc;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_boot_manager::LenovoBootManagerUpdate;

/// Category of Lenovo persistent boot order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootOrderKind {
    /// Top-level device category order.
    General,
    /// Optical-device order.
    CdDvdRom,
    /// Hard-disk order.
    HardDisk,
    /// Network-device order.
    Network,
    /// USB-device order.
    Usb,
}

impl BootOrderKind {
    /// Return the Lenovo resource identifier for this category.
    const fn resource_id(self) -> &'static str {
        match self {
            Self::General => "BootOrder.BootOrder",
            Self::CdDvdRom => "BootOrder.CDDVDROMBootOrder",
            Self::HardDisk => "BootOrder.HardDiskBootOrder",
            Self::Network => "BootOrder.NetworkBootOrder",
            Self::Usb => "BootOrder.USBBootOrder",
        }
    }
}

/// Collection of Lenovo persistent boot-order resources.
pub struct LenovoBootManagerCollection<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<LenovoBootManagerCollectionSchema>,
}

impl<B: Bmc> LenovoBootManagerCollection<B> {
    /// Create a collection from an advertised navigation property.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<LenovoBootManagerCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Fetch one advertised boot-order category.
    ///
    /// Returns `Ok(None)` when the collection does not advertise the selected
    /// category.
    ///
    /// # Errors
    ///
    /// Returns an error if the selected resource cannot be fetched.
    pub async fn boot_order(
        &self,
        kind: BootOrderKind,
    ) -> Result<Option<LenovoBootManager<B>>, Error<B>> {
        let member = self.data.members.as_ref().and_then(|members| {
            members
                .iter()
                .find(|member| member.id().last_segment() == Some(kind.resource_id()))
        });
        match member {
            Some(member) => LenovoBootManager::new(&self.bmc, member).await.map(Some),
            None => Ok(None),
        }
    }

    /// Fetch all boot-order resources advertised by this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if a collection member cannot be fetched.
    pub async fn members(&self) -> Result<Vec<LenovoBootManager<B>>, Error<B>> {
        let Some(members) = self.data.members.as_ref() else {
            return Ok(Vec::new());
        };
        let mut result = Vec::with_capacity(members.len());
        for member in members {
            result.push(LenovoBootManager::new(&self.bmc, member).await?);
        }
        Ok(result)
    }

    /// Get the raw Lenovo boot manager collection schema.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoBootManagerCollectionSchema> {
        self.data.clone()
    }
}

/// Lenovo persistent boot-order resource.
pub struct LenovoBootManager<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<LenovoBootManagerSchema>,
}

impl<B: Bmc> LenovoBootManager<B> {
    /// Create a boot-order resource from an advertised collection member.
    async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<LenovoBootManagerSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Current persistent boot order.
    #[must_use]
    pub fn current(&self) -> Option<&[String]> {
        self.data
            .boot_order_current
            .as_ref()
            .and_then(Option::as_deref)
    }

    /// Pending persistent boot order.
    #[must_use]
    pub fn next(&self) -> Option<&[String]> {
        self.data
            .boot_order_next
            .as_ref()
            .and_then(Option::as_deref)
    }

    /// Values supported by this boot-order resource.
    #[must_use]
    pub fn supported(&self) -> Option<&[String]> {
        self.data
            .boot_order_supported
            .as_ref()
            .and_then(Option::as_deref)
    }

    /// Update the pending persistent boot order.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned resource fails.
    pub async fn update(
        &self,
        update: &LenovoBootManagerUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<LenovoBootManagerSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Get the raw Lenovo boot manager schema.
    #[must_use]
    pub fn raw(&self) -> Arc<LenovoBootManagerSchema> {
        self.data.clone()
    }
}
