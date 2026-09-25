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

//! Supermicro ComputerSystem OEM extension.

use std::sync::Arc;

use crate::core::Bmc;
use crate::oem::oem_object;
use crate::oem::supermicro::fixed_boot_order::SmcFixedBootOrder;
use crate::oem::supermicro::schema::smc_system_extensions::System as SupermicroComputerSystemSchema;
use crate::schema::computer_system::ComputerSystem as ComputerSystemSchema;
use crate::Error;
use crate::NvBmc;

/// Supermicro properties advertised by a ComputerSystem.
pub struct SupermicroComputerSystem<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SupermicroComputerSystemSchema>,
}

impl<B: Bmc> SupermicroComputerSystem<B> {
    /// Parse Supermicro data from a ComputerSystem OEM object.
    ///
    /// Returns `Ok(None)` when the system does not advertise `Oem.Supermicro`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing Supermicro system data fails.
    pub(crate) fn new(
        bmc: &NvBmc<B>,
        system: &ComputerSystemSchema,
    ) -> Result<Option<Self>, Error<B>> {
        Ok(system
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Supermicro"))?
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            }))
    }

    /// Get the raw Supermicro ComputerSystem data.
    #[must_use]
    pub fn raw(&self) -> Arc<SupermicroComputerSystemSchema> {
        self.data.clone()
    }

    /// Fetch the advertised Supermicro fixed boot order.
    ///
    /// Returns `Ok(None)` when no fixed boot order is advertised.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the fixed boot order fails.
    pub async fn fixed_boot_order(&self) -> Result<Option<SmcFixedBootOrder<B>>, Error<B>> {
        if let Some(nav) = &self.data.fixed_boot_order {
            SmcFixedBootOrder::new(&self.bmc, nav).await.map(Some)
        } else {
            Ok(None)
        }
    }
}
