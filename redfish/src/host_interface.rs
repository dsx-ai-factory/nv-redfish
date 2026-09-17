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

//! Host interfaces
//!

use crate::schema::host_interface::HostInterface as HostInterfaceSchema;
use crate::schema::host_interface_collection::HostInterfaceCollection as HostInterfaceCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::host_interface::HostInterfaceUpdate;

/// Host interfaces collection.
///
/// Provides functions to access collection members.
pub struct HostInterfaceCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<HostInterfaceCollectionSchema>,
}

impl<B: Bmc> HostInterfaceCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<HostInterfaceCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = bmc.expand_property(nav).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List all managers available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching manager data fails.
    pub async fn members(&self) -> Result<Vec<HostInterface<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(HostInterface::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

/// Host Interface.
///
/// Provides functions to access host interface.
pub struct HostInterface<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<HostInterfaceSchema>,
}

impl<B: Bmc> HostInterface<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<HostInterfaceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this host interface.
    #[must_use]
    pub fn raw(&self) -> Arc<HostInterfaceSchema> {
        self.data.clone()
    }

    /// Update this host interface.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &HostInterfaceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<HostInterfaceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// State of the interface. `None` means that BMC hasn't reported
    /// interface state or reported null.
    #[must_use]
    pub fn interface_enabled(&self) -> Option<bool> {
        self.data
            .interface_enabled
            .as_ref()
            .and_then(Option::as_ref)
            .copied()
    }
}
