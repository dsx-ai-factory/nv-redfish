// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell job-service resource and actions.

use std::sync::Arc;

use crate::core::{Bmc, ModificationResponse, NavProperty};
use crate::oem::dell::schema::dell_job_service::{
    DellJobService as DellJobServiceSchema, DellJobServiceDeleteJobQueueAction,
};
use crate::{Error, NvBmc};

/// Dell job-service handle.
pub struct DellJobService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<DellJobServiceSchema>,
}

impl<B: Bmc> DellJobService<B> {
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
    /// Returns [`Error::ActionNotAvailable`] when the service does not
    /// advertise the action, or a BMC error if invocation fails.
    pub async fn delete_job_queue(
        &self,
        job_id: impl Into<String>,
    ) -> Result<ModificationResponse<()>, Error<B>> {
        let action = self
            .data
            .actions
            .as_ref()
            .and_then(Option::as_ref)
            .and_then(|actions| actions.delete_job_queue.as_ref())
            .ok_or(Error::ActionNotAvailable)?;
        action
            .run(
                self.bmc.as_ref(),
                &DellJobServiceDeleteJobQueueAction {
                    job_id: job_id.into(),
                },
            )
            .await
            .map_err(Error::Bmc)
    }

    /// Get the raw Dell job-service schema.
    #[must_use]
    pub fn raw(&self) -> Arc<DellJobServiceSchema> {
        self.data.clone()
    }
}
