// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell job-service resource and actions.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::{
    Action, Bmc, EntityTypeRef, ModificationResponse, NavProperty, ODataETag, ODataId,
};
use crate::{Error, NvBmc};

/// Schema for the Dell job-service resource.
#[derive(Debug, Deserialize)]
pub struct DellJobServiceSchema {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
    #[serde(rename = "@odata.etag", default)]
    etag: Option<ODataETag>,
    #[serde(rename = "Actions")]
    actions: DellJobServiceActions,
}

impl EntityTypeRef for DellJobServiceSchema {
    fn odata_id(&self) -> &ODataId {
        &self.odata_id
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.etag.as_ref()
    }
}

#[derive(Debug, Deserialize)]
struct DellJobServiceActions {
    #[serde(rename = "#DellJobService.DeleteJobQueue")]
    delete_job_queue: Option<Action<DeleteJobQueue, ()>>,
}

#[derive(Debug, Serialize)]
struct DeleteJobQueue {
    #[serde(rename = "JobID")]
    job_id: String,
}

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

    /// Delete one job, or clear the queue with the Dell `JID_CLEARALL` sentinel.
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
            .delete_job_queue
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        action
            .run(
                self.bmc.as_ref(),
                &DeleteJobQueue {
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
