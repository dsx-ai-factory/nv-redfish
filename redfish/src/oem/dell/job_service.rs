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

//! Dell job-service resource and actions.

use std::sync::Arc;

#[cfg(feature = "managers")]
use crate::core::NavProperty;
use crate::core::{ActionError, Bmc, ModificationResponse};
use crate::oem::dell::schema::dell_job_service::DellJobService as DellJobServiceSchema;
use crate::{Error, NvBmc};

/// Dell job-service handle.
pub struct DellJobService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<DellJobServiceSchema>,
}

impl<B: Bmc> DellJobService<B> {
    /// Fetch a Dell JobService from an advertised Manager link.
    #[cfg(feature = "managers")]
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<DellJobServiceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Delete one job, or clear the queue and pending attribute values with
    /// the Dell `JID_CLEARALL` sentinel.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the service omits its
    /// Actions container, or a BMC error if the generated action helper
    /// reports the action unsupported or invocation fails.
    pub async fn delete_job_queue(
        &self,
        job_id: impl Into<String>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .and_then(Option::as_ref)
            .ok_or(Error::ActionNotAvailable)?;
        actions
            .delete_job_queue(self.bmc.as_ref(), job_id.into())
            .await
            .map_err(Error::Bmc)
    }

    /// Get the raw Dell job-service schema.
    #[must_use]
    pub fn raw(&self) -> Arc<DellJobServiceSchema> {
        self.data.clone()
    }
}
