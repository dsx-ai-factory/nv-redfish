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

//! Assembly
//!

use crate::hardware_id::HardwareIdRef;
use crate::hardware_id::Manufacturer as HardwareIdManufacturer;
use crate::hardware_id::Model as HardwareIdModel;
use crate::hardware_id::PartNumber as HardwareIdPartNumber;
use crate::hardware_id::SerialNumber as HardwareIdSerialNumber;
use crate::schema::assembly::Assembly as AssemblySchema;
use crate::schema::assembly::AssemblyData as AssemblyDataSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::marker::PhantomData;
use std::sync::Arc;

#[doc(hidden)]
pub enum AssemblyTag {}

/// Assembly manufacturer (AKA Producer).
pub type Manufacturer<T> = HardwareIdManufacturer<T, AssemblyTag>;

/// Assembly model.
pub type Model<T> = HardwareIdModel<T, AssemblyTag>;

/// Assembly part number.
pub type PartNumber<T> = HardwareIdPartNumber<T, AssemblyTag>;

/// Assembly number.
pub type SerialNumber<T> = HardwareIdSerialNumber<T, AssemblyTag>;

/// Assembly.
///
/// Provides functions to access assembly.
pub struct Assembly<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<AssemblySchema>,
}

impl<B: Bmc> Assembly<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<AssemblySchema>,
    ) -> Result<Self, Error<B>> {
        // We use expand here becuase Assembly/Assemblies are
        // navigation properties, so we want to take them using one
        // get.
        bmc.expand_property(nav).await.map(|data| Self {
            bmc: bmc.clone(),
            data,
        })
    }

    /// Get the raw schema data for this assembly.
    #[must_use]
    pub fn raw(&self) -> Arc<AssemblySchema> {
        self.data.clone()
    }

    /// Get assemblies.
    ///
    /// # Errors
    ///
    /// Returns error if this assembly was not expanded by initial get
    /// and then function failed to get data of the assembly.
    pub async fn assemblies(&self) -> Result<Vec<AssemblyData<B>>, Error<B>> {
        let mut result = Vec::new();
        if let Some(assemblies) = &self.data.assemblies {
            for m in assemblies {
                result.push(AssemblyData::new(&self.bmc, m).await?);
            }
        }
        Ok(result)
    }
}

/// Assembly data.
pub struct AssemblyData<B: Bmc> {
    data: Arc<AssemblyDataSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> AssemblyData<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<AssemblyDataSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this assembly.
    #[must_use]
    pub fn raw(&self) -> Arc<AssemblyDataSchema> {
        self.data.clone()
    }

    /// Get hardware identifier of the network adpater.
    #[must_use]
    pub fn hardware_id(&self) -> HardwareIdRef<'_, AssemblyTag> {
        HardwareIdRef {
            manufacturer: self
                .data
                .producer
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
}
