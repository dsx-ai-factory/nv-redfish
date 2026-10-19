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

//! Dell actions advertised by a Storage resource.

use std::sync::Arc;

use serde::Deserialize as _;

use crate::core::action::Action;
use crate::core::{AsyncTask, Bmc, ModificationResponse, ReferenceLeaf};
use crate::oem::dell::schema::dell_storage::StorageControllerDrivesDecommissionAction;
use crate::oem::dell::schema::storage::OemActions as DellStorageOemActions;
use crate::oem::dell::schema::ActionAnnotations;
use crate::schema::storage::OemActions as StorageOemActions;
use crate::{Error, NvBmc};

pub use crate::oem::dell::schema::settings::OperationApplyTime;

/// Dell actions advertised by a Storage resource.
pub struct DellStorageActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<DellStorageOemActions>,
}

impl<B: Bmc> DellStorageActions<B> {
    /// Parse Dell actions from a standard Storage OEM actions object.
    pub(crate) fn new(bmc: &NvBmc<B>, actions: &StorageOemActions) -> Result<Self, Error<B>> {
        DellStorageOemActions::deserialize(&actions.additional_properties)
            .map_err(Error::Json)
            .map(|data| Self {
                bmc: bmc.clone(),
                data: Arc::new(data),
            })
    }

    /// Decommission every drive attached to the controller.
    ///
    /// A job `Location` returned with a 200 or 201 is returned as a task.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the Storage resource does
    /// not advertise this action, or a BMC error if invocation fails.
    pub async fn decommission_controller_drives(
        &self,
        apply_time: Option<OperationApplyTime>,
    ) -> Result<ModificationResponse<()>, Error<B>> {
        let action = self
            .data
            .controller_drives_decommission
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;

        let redfish_annotations = ActionAnnotations {
            operation_apply_time: apply_time,
        };
        self.bmc
            .as_ref()
            .action::<_, ReferenceLeaf>(
                &Action::new(action.target.clone()),
                &StorageControllerDrivesDecommissionAction {
                    redfish_annotations,
                },
            )
            .await
            .map(|response| match response {
                // iDRAC can report the scheduled job as 200/201 with its URI
                // in Location or as the body's `@odata.id`; expose it as
                // asynchronous work to callers.
                ModificationResponse::Entity(job) => ModificationResponse::Task(AsyncTask {
                    location: job.odata_id.into(),
                    retry_after: None,
                    task: None,
                }),
                ModificationResponse::Task(task) => ModificationResponse::Task(task),
                ModificationResponse::Empty => ModificationResponse::Empty,
            })
            .map_err(Error::from)
    }
}
