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

//! Serial interfaces of a manager, such as the one used for Serial over LAN.

use crate::schema::serial_interface::SerialInterface as SerialInterfaceSchema;
use crate::schema::serial_interface_collection::SerialInterfaceCollection as SerialInterfaceCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::serial_interface::SerialInterfaceUpdate;

/// Serial interfaces collection.
///
/// Provides functions to access collection members.
pub struct SerialInterfaceCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<SerialInterfaceCollectionSchema>,
}

impl<B: Bmc> SerialInterfaceCollection<B> {
    /// Create a new serial interface collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SerialInterfaceCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = bmc.expand_property(nav).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List the serial interfaces in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching a serial interface fails.
    pub async fn members(&self) -> Result<Vec<SerialInterface<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(SerialInterface::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

/// Serial interface.
///
/// Provides functions to access a serial interface.
pub struct SerialInterface<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SerialInterfaceSchema>,
}

impl<B: Bmc> SerialInterface<B> {
    /// Create a new serial interface handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SerialInterfaceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this serial interface.
    #[must_use]
    pub fn raw(&self) -> Arc<SerialInterfaceSchema> {
        self.data.clone()
    }

    /// Update this serial interface.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &SerialInterfaceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<SerialInterfaceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
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
