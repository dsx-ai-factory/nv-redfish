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
use nv_redfish_core::NavProperty;
use std::marker::PhantomData;
use std::sync::Arc;

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
    data: Arc<HostInterfaceSchema>,
    _marker: PhantomData<B>,
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
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this host interface.
    #[must_use]
    pub fn raw(&self) -> Arc<HostInterfaceSchema> {
        self.data.clone()
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
