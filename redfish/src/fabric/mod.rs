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

mod item;
mod switch;

use crate::core::NavProperty;
use crate::patch_support::CollectionWithPatch;
use crate::schema::fabric::Fabric as FabricSchema;
use crate::schema::fabric_collection::FabricCollection as FabricCollectionSchema;
use crate::schema::resource::ResourceCollection;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use std::sync::Arc;

#[doc(inline)]
pub use item::Fabric;
#[doc(inline)]
pub use item::FabricLink;
#[doc(inline)]
pub use switch::FirmwareVersion;
#[doc(inline)]
pub use switch::Manufacturer;
#[doc(inline)]
pub use switch::Model;
#[doc(inline)]
pub use switch::PartNumber;
#[doc(inline)]
pub use switch::SerialNumber;
#[doc(inline)]
pub use switch::Sku;
#[doc(inline)]
pub use switch::Switch;
#[doc(inline)]
pub use switch::SwitchCollection;
#[doc(inline)]
pub use switch::SwitchLink;
#[doc(inline)]
pub use switch::SwitchTag;

#[doc(inline)]
pub use crate::schema::protocol::Protocol;

/// Fabric collection.
///
/// Provides functions to access collection members.
pub struct FabricCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<FabricCollectionSchema>,
}

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
