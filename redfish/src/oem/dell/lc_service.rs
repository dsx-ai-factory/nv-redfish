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

//! Dell Lifecycle Controller service advertised by a Manager.

use std::sync::Arc;

use crate::core::{ActionError, Bmc, ModificationResponse, NavProperty};
use crate::oem::dell::schema::dell_lc_service::DellLcService as DellLcServiceSchema;
use crate::oem::dell::schema::dell_lc_service::GetRemoteServicesApiStatusResponse;
use crate::{Error, NvBmc};

/// Dell Lifecycle Controller service handle.
pub struct DellLcService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<DellLcServiceSchema>,
}

impl<B: Bmc> DellLcService<B> {
    /// Fetch the Lifecycle Controller service from an advertised Manager link.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<DellLcServiceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the remote services API status, including whether the Lifecycle
    /// Controller is ready to accept configuration requests (`LCStatus`).
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the service omits its
    /// Actions container, or a BMC error if the generated action helper
    /// reports the action unsupported or invocation fails.
    pub async fn remote_services_api_status(
        &self,
    ) -> Result<ModificationResponse<GetRemoteServicesApiStatusResponse>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        actions
            .get_remote_services_api_status(self.bmc.as_ref())
            .await
            .map_err(Error::from)
    }

    /// Get the raw Dell Lifecycle Controller service schema.
    #[must_use]
    pub fn raw(&self) -> Arc<DellLcServiceSchema> {
        self.data.clone()
    }
}
