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
//! Manager network protocol resource.

use std::sync::Arc;

use nv_redfish_core::{Bmc, EntityTypeRef as _, ModificationResponse, NavProperty};

use crate::schema::manager_network_protocol::ManagerNetworkProtocol as ManagerNetworkProtocolSchema;
use crate::{Error, NvBmc};

#[doc(inline)]
pub use crate::schema::manager_network_protocol::ManagerNetworkProtocolUpdate;

/// Network protocol configuration associated with a manager.
pub struct ManagerNetworkProtocol<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<ManagerNetworkProtocolSchema>,
}

impl<B: Bmc> ManagerNetworkProtocol<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ManagerNetworkProtocolSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for the manager network protocol resource.
    #[must_use]
    pub fn raw(&self) -> Arc<ManagerNetworkProtocolSchema> {
        self.data.clone()
    }

    /// Update this manager network protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &ManagerNetworkProtocolUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<ManagerNetworkProtocolSchema>>(
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
