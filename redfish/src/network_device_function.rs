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

//! Network device functions.

use crate::mac_address::MacAddress;
use crate::schema::network_device_function::NetworkDeviceFunction as NetworkDeviceFunctionSchema;
use crate::schema::network_device_function_collection::NetworkDeviceFunctionCollection as NetworkDeviceFunctionCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::marker::PhantomData;
use std::sync::Arc;

/// Network device functions collection.
///
/// Provides functions to access collection members.
pub struct NetworkDeviceFunctionCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<NetworkDeviceFunctionCollectionSchema>,
}

impl<B: Bmc> NetworkDeviceFunctionCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<NetworkDeviceFunctionCollectionSchema>,
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
    pub async fn members(&self) -> Result<Vec<NetworkDeviceFunction<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(NetworkDeviceFunction::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

/// Network device function.
///
/// Provides functions to access network device function.
pub struct NetworkDeviceFunction<B: Bmc> {
    data: Arc<NetworkDeviceFunctionSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> NetworkDeviceFunction<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<NetworkDeviceFunctionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this network device function.
    #[must_use]
    pub fn raw(&self) -> Arc<NetworkDeviceFunctionSchema> {
        self.data.clone()
    }

    /// The permanent MAC address assigned to this function.
    pub fn ethernet_permanent_mac_address(&self) -> Option<MacAddress<'_>> {
        self.data
            .ethernet
            .as_ref()
            .and_then(|eth| eth.permanent_mac_address.as_ref())
            .and_then(Option::as_deref)
            .map(MacAddress::new)
    }
}
