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

//! Manager entities and collections.
//!
//! This module provides types for working with Redfish Manager resources.

mod item;
#[cfg(feature = "manager-network-protocol")]
mod network_protocol;

use crate::core::NavProperty;
use crate::patch_support::CollectionWithPatch;
use crate::patch_support::FilterFn;
use crate::patch_support::JsonValue;
use crate::schema::manager::Manager as ManagerSchema;
use crate::schema::manager_collection::ManagerCollection as ManagerCollectionSchema;
use crate::schema::resource::ResourceCollection;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use std::convert::identity;
use std::sync::Arc;

pub use item::Manager;
#[cfg(feature = "manager-network-protocol")]
pub use network_protocol::ManagerNetworkProtocol;
#[cfg(feature = "manager-network-protocol")]
pub use network_protocol::ManagerNetworkProtocolUpdate;

#[doc(inline)]
pub use crate::schema::manager::ResetToDefaultsType as ManagerResetToDefaultsType;

/// Manager collection.
///
/// Provides functions to access collection members.
pub struct ManagerCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<ManagerCollectionSchema>,
}

impl<B: Bmc> ManagerCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let mut filters = Vec::new();
        if let Some(odata_id_filter) = bmc.quirks.filter_manager_odata_ids() {
            filters.push(Box::new(move |js: &JsonValue| {
                js.get("@odata.id")
                    .and_then(|v| v.as_str())
                    .map(odata_id_filter)
                    .is_some_and(identity)
            }));
        }
        let filters_fn = (!filters.is_empty())
            .then(move || Arc::new(move |v: &JsonValue| filters.iter().any(|f| f(v))) as FilterFn);

        if let Some(collection_ref) = &root.root.managers {
            Self::expand_collection(bmc, collection_ref, None, filters_fn.as_ref())
                .await
                .map(Some)
        } else if bmc.quirks.bug_missing_root_nav_properties() {
            bmc.expand_property(&NavProperty::new_reference(
                format!("{}/Managers", root.root.odata_id).into(),
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
            })
        })
    }

    /// List all managers available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching manager data fails.
    pub async fn members(&self) -> Result<Vec<Manager<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(Manager::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

impl<B: Bmc> CollectionWithPatch<ManagerCollectionSchema, ManagerSchema, B>
    for ManagerCollection<B>
{
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<ManagerSchema>>,
    ) -> ManagerCollectionSchema {
        ManagerCollectionSchema {
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
