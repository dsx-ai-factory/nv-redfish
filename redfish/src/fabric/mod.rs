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

//! Fabric entities and collections.
//!
//! This module provides types for working with Redfish Fabric
//! resources and the switches they contain.

#[cfg(feature = "fabrics")]
mod item;
#[cfg(feature = "switches")]
mod switch;

#[cfg(feature = "fabrics")]
use crate::core::NavProperty;
#[cfg(feature = "fabrics")]
use crate::patch_support::CollectionWithPatch;
#[cfg(feature = "fabrics")]
use crate::schema::fabric::Fabric as FabricSchema;
#[cfg(feature = "fabrics")]
use crate::schema::fabric_collection::FabricCollection as FabricCollectionSchema;
#[cfg(feature = "fabrics")]
use crate::schema::resource::ResourceCollection;
#[cfg(feature = "fabrics")]
use crate::Error;
#[cfg(feature = "fabrics")]
use crate::NvBmc;
#[cfg(feature = "fabrics")]
use crate::ServiceRoot;
#[cfg(feature = "fabrics")]
use nv_redfish_core::Bmc;
#[cfg(feature = "fabrics")]
use std::sync::Arc;

#[cfg(feature = "fabrics")]
#[doc(inline)]
pub use item::Fabric;
#[cfg(feature = "fabrics")]
#[doc(inline)]
pub use item::FabricLink;

#[cfg(feature = "switches")]
#[doc(inline)]
pub use switch::{
    FirmwareVersion, Manufacturer, Model, PartNumber, SerialNumber, Sku, Switch, SwitchCollection,
    SwitchFirmwareVersionTag, SwitchLink, SwitchSkuTag, SwitchTag,
};

#[doc(inline)]
pub use crate::schema::protocol::Protocol;

/// Fabric collection.
///
/// Provides functions to access collection members.
#[cfg(feature = "fabrics")]
pub struct FabricCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<FabricCollectionSchema>,
}

#[cfg(feature = "fabrics")]
impl<B: Bmc> FabricCollection<B> {
    /// Create a new fabric collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(collection_ref) = &root.root.fabrics else {
            return Ok(None);
        };
        let collection = Self::expand_collection(bmc, collection_ref, None, None).await?;
        Ok(Some(Self {
            bmc: bmc.clone(),
            collection,
        }))
    }

    /// List all fabrics available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching fabric data fails.
    pub async fn members(&self) -> Result<Vec<Fabric<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(Fabric::new(&self.bmc, m).await?);
        }
        Ok(members)
    }

    /// Return lazy links for the fabrics in this collection.
    ///
    /// Each link can be fetched independently, allowing callers to choose their own error-handling
    /// policy without changing the eager, all-or-nothing behavior of [`Self::members`].
    #[must_use]
    pub fn member_links(&self) -> Vec<FabricLink<B>> {
        self.collection
            .members
            .iter()
            .map(|member| {
                FabricLink::new(&self.bmc, NavProperty::new_reference(member.id().clone()))
            })
            .collect()
    }
}

#[cfg(feature = "fabrics")]
impl<B: Bmc> CollectionWithPatch<FabricCollectionSchema, FabricSchema, B> for FabricCollection<B> {
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<FabricSchema>>,
    ) -> FabricCollectionSchema {
        FabricCollectionSchema {
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
