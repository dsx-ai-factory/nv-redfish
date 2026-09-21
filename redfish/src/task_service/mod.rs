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

//! Task Service entities and helpers.
//!
//! This module provides typed access to Redfish `TaskService`.
//! A `TaskService` value is a lightweight handle to the service schema and BMC
//! transport. It validates task locations returned by asynchronous operations
//! against this service's Tasks collection and returns lazy task links that can
//! be fetched when polling is needed.

use std::sync::Arc;

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::NavProperty;
use crate::entity_link::EntityLink;
use crate::schema::task::Task as TaskSchema;
use crate::schema::task_service::TaskService as TaskServiceSchema;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;

use nv_redfish_core::AsyncTask;

/// Link to a Redfish Task returned by an asynchronous operation.
pub type TaskLink<B> = EntityLink<B, TaskSchema>;

/// Task service.
///
/// Provides task links for task locations returned by asynchronous operations.
///
/// # Example
///
/// ```ignore
/// let Some(task_service) = root.task_service().await? else {
///     return Ok(());
/// };
///
/// let task_link = task_service.task_link(&async_task)?;
/// let task = task_link.fetch().await?;
///
/// println!("{:?}", task.task_state);
/// ```
pub struct TaskService<B: Bmc> {
    data: Arc<TaskServiceSchema>,
    bmc: NvBmc<B>,
}

impl<B: Bmc> TaskService<B> {
    /// Create a new task service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(service_ref) = &root.root.tasks else {
            return Ok(None);
        };

        let data = service_ref.get(bmc.as_ref()).await.map_err(Error::Bmc)?;

        // Task links need the BMC-advertised Tasks collection as the allowed
        // parent path for all async task locations.
        if data.tasks.is_none() {
            return Err(Error::TaskServiceTasksUnavailable);
        }

        Ok(Some(Self {
            data,
            bmc: bmc.clone(),
        }))
    }

    /// Get the raw schema data for this task service.
    #[must_use]
    pub fn raw(&self) -> Arc<TaskServiceSchema> {
        self.data.clone()
    }

    /// Create a Task resource link from an asynchronous operation result.
    ///
    /// The Task resource from the response body is preferred when present,
    /// because the response `Location` can identify a task monitor rather than
    /// a Task resource. The selected URI must be a child of this service's
    /// Tasks collection, such as `/redfish/v1/TaskService/Tasks/{id}`. The
    /// returned link does not fetch the task until [`TaskLink::fetch`] is
    /// called.
    ///
    /// # Errors
    ///
    /// Returns error if the task location is not a child of this service's Tasks
    /// collection.
    pub fn task_link(&self, task: &AsyncTask) -> Result<TaskLink<B>, Error<B>> {
        let Some(tasks) = self.data.tasks.as_ref() else {
            return Err(Error::TaskServiceTasksUnavailable);
        };

        let task_collection = tasks.odata_id();
        let task_location = task.task_status_uri().clone();
        if task_collection == &task_location || !task_collection.is_path_prefix(&task_location) {
            return Err(Error::TaskLocationNotInTaskService {
                task_location,
                task_collection: task_collection.clone(),
            });
        }

        let task_ref = NavProperty::new_reference(task_location);
        Ok(TaskLink::new(&self.bmc, task_ref))
    }
}
