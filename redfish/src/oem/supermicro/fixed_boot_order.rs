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

//! Supermicro fixed boot order OEM resource.

use std::sync::Arc;

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
pub use crate::oem::supermicro::schema::smc_fixed_boot_order::BootMode;
use crate::oem::supermicro::schema::smc_fixed_boot_order::SmcFixedBootOrder as SmcFixedBootOrderSchema;
pub use crate::oem::supermicro::schema::smc_fixed_boot_order::SmcFixedBootOrderUpdate;
use crate::Error;
use crate::NvBmc;

/// Supermicro fixed boot order handle.
pub struct SmcFixedBootOrder<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SmcFixedBootOrderSchema>,
}

impl<B: Bmc> SmcFixedBootOrder<B> {
    /// Fetch the fixed boot order resource from an advertised link.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the resource fails.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SmcFixedBootOrderSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw fixed boot order schema data.
    #[must_use]
    pub fn raw(&self) -> Arc<SmcFixedBootOrderSchema> {
        self.data.clone()
    }

    /// Selected firmware boot mode.
    #[must_use]
    pub fn boot_mode_selected(&self) -> Option<BootMode> {
        self.data.boot_mode_selected
    }

    /// Device-class boot order.
    #[must_use]
    pub fn fixed_boot_order(&self) -> Option<&[String]> {
        self.data.fixed_boot_order.as_deref()
    }

    /// Disabled device-class entries.
    #[must_use]
    pub fn fixed_boot_order_disabled_items(&self) -> Option<&[String]> {
        self.data.fixed_boot_order_disabled_item.as_deref()
    }

    /// UEFI hard-disk device order.
    #[must_use]
    pub fn uefi_hard_disk(&self) -> Option<&[String]> {
        self.data.uefi_hard_disk.as_deref()
    }

    /// Disabled UEFI hard-disk entries.
    #[must_use]
    pub fn uefi_hard_disk_disabled_items(&self) -> Option<&[String]> {
        self.data.uefi_hard_disk_disabled_item.as_deref()
    }

    /// UEFI network device order.
    #[must_use]
    pub fn uefi_network(&self) -> Option<&[String]> {
        self.data.uefi_network.as_deref()
    }

    /// Disabled UEFI network entries.
    #[must_use]
    pub fn uefi_network_disabled_items(&self) -> Option<&[String]> {
        self.data.uefi_network_disabled_item.as_deref()
    }

    /// Update this fixed boot order resource.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &SmcFixedBootOrderUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<SmcFixedBootOrderSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }
}
