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

//! Network adapters

use crate::hardware_id::HardwareIdRef;
use crate::hardware_id::Manufacturer as HardwareIdManufacturer;
use crate::hardware_id::Model as HardwareIdModel;
use crate::hardware_id::PartNumber as HardwareIdPartNumber;
use crate::hardware_id::SerialNumber as HardwareIdSerialNumber;
use crate::schema::network_adapter::NetworkAdapter as NetworkAdapterSchema;
use crate::schema::network_adapter_collection::NetworkAdapterCollection as NetworkAdapterCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

#[cfg(feature = "network-device-functions")]
use crate::network_device_function::NetworkDeviceFunctionCollection;
#[cfg(feature = "ports")]
use crate::port::PortCollection;

/// Network adapters collection.
///
/// Provides functions to access collection members.
pub struct NetworkAdapterCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<NetworkAdapterCollectionSchema>,
}

impl<B: Bmc> NetworkAdapterCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<NetworkAdapterCollectionSchema>,
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
    pub async fn members(&self) -> Result<Vec<NetworkAdapter<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(NetworkAdapter::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

#[doc(hidden)]
pub enum NetworkAdapterTag {}

/// Network adapter manufacturer.
pub type Manufacturer<T> = HardwareIdManufacturer<T, NetworkAdapterTag>;

/// Network adapter model.
pub type Model<T> = HardwareIdModel<T, NetworkAdapterTag>;

/// Network adapter part number.
pub type PartNumber<T> = HardwareIdPartNumber<T, NetworkAdapterTag>;

/// Network adapter serial number.
pub type SerialNumber<T> = HardwareIdSerialNumber<T, NetworkAdapterTag>;

/// Network Adapter.
///
/// Provides functions to access log entries and perform log operations.
pub struct NetworkAdapter<B: Bmc> {
    #[allow(dead_code)] // used if any feature enabled.
    bmc: NvBmc<B>,
    data: Arc<NetworkAdapterSchema>,
}

impl<B: Bmc> NetworkAdapter<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<NetworkAdapterSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this ethernet adapter.
    #[must_use]
    pub fn raw(&self) -> Arc<NetworkAdapterSchema> {
        self.data.clone()
    }

    /// Get hardware identifier of the network adpater.
    #[must_use]
    pub fn hardware_id(&self) -> HardwareIdRef<'_, NetworkAdapterTag> {
        HardwareIdRef {
            manufacturer: self
                .data
                .manufacturer
                .as_ref()
                .and_then(Option::as_deref)
                .map(Manufacturer::new),
            model: self
                .data
                .model
                .as_ref()
                .and_then(Option::as_deref)
                .map(Model::new),
            part_number: self
                .data
                .part_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(PartNumber::new),
            serial_number: self
                .data
                .serial_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(SerialNumber::new),
        }
    }

    /// Get network device functions for this adapter.
    ///
    /// Returns `Ok(None)` when the network device functions link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching network device functions data fails.
    #[cfg(feature = "network-device-functions")]
    pub async fn network_device_functions(
        &self,
    ) -> Result<Option<NetworkDeviceFunctionCollection<B>>, Error<B>> {
        if let Some(p) = &self.data.network_device_functions {
            NetworkDeviceFunctionCollection::new(&self.bmc, p)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get ports for this adapter.
    ///
    /// Returns `Ok(None)` when the ports link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the port collection fails.
    #[cfg(feature = "ports")]
    pub async fn ports(&self) -> Result<Option<PortCollection<B>>, Error<B>> {
        if let Some(ports) = &self.data.ports {
            PortCollection::new(&self.bmc, ports).await.map(Some)
        } else {
            Ok(None)
        }
    }
}
