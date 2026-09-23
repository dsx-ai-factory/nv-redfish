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

//! Support Supermicro KCS Interface OEM resource.

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::oem::supermicro::schema::kcs_interface::KcsInterface as KcsInterfaceSchema;
pub use crate::oem::supermicro::schema::kcs_interface::KcsInterfaceUpdate;
use crate::Error;
use crate::NvBmc;
use std::sync::Arc;

#[doc(inline)]
pub use crate::oem::supermicro::schema::kcs_interface::Privilege;

/// Supermicro KCS interface resource.
pub struct KcsInterface<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<KcsInterfaceSchema>,
}

impl<B: Bmc> KcsInterface<B> {
    /// Create a Supermicro KCS interface wrapper.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching KCS interface data fails.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<KcsInterfaceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this Supermicro KCS interface.
    #[must_use]
    pub fn raw(&self) -> Arc<KcsInterfaceSchema> {
        self.data.clone()
    }

    /// Privilege associated with this KCS interface.
    #[must_use]
    pub fn privilege(&self) -> Option<Privilege> {
        self.data.privilege
    }

    /// Update this KCS interface.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &KcsInterfaceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<KcsInterfaceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Set the privilege granted through KCS.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn set_privilege(
        &self,
        privilege: Privilege,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update(
            &KcsInterfaceUpdate::builder()
                .with_privilege(privilege)
                .build(),
        )
        .await
    }
}
