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

use std::mem;
use std::sync::Arc;
use std::time::Duration;

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::core::ODataId;
use crate::core::OperationResponseBmc;
use crate::entity_link::EntityLink;
use crate::schema::task::Task as TaskSchema;
use crate::schema::task_service::TaskService as TaskServiceSchema;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;

use nv_redfish_core::AsyncTask;

/// Link to a Redfish Task returned by an asynchronous operation.
pub type TaskLink<B> = EntityLink<B, TaskSchema>;

enum State<R> {
    /// The result arrived with the original response.
    Ready(R),
    /// The operation is running.
    Pending(AsyncTask),
    /// The operation finished; its result must be read from the
    /// compatibility result URI.
    Finished,
    /// An earlier poll returned the outcome.
    Done,
}

/// A modification response known to return a typed result, possibly
/// asynchronously.
#[must_use = "typed modification responses must be polled"]
pub struct TypedModificationResponse<R> {
    state: State<R>,
    compatibility_result: Option<ODataId>,
}

impl<R> TypedModificationResponse<R> {
    /// Create a typed response from an action with a response payload.
    pub fn from_typed_action(response: ModificationResponse<R>) -> Self {
        Self::new(response, None)
    }

    /// Create a typed response whose result is read from `result_location`
    /// when the service completes without returning it.
    pub fn with_compatibility_result(
        response: ModificationResponse<R>,
        result_location: ODataId,
    ) -> Self {
        Self::new(response, Some(result_location))
    }

    fn new(response: ModificationResponse<R>, compatibility_result: Option<ODataId>) -> Self {
        let state = match response {
            ModificationResponse::Entity(result) => State::Ready(result),
            ModificationResponse::Task(task) => State::Pending(task),
            ModificationResponse::Empty => State::Finished,
        };
        Self {
            state,
            compatibility_result,
        }
    }

    /// Recommended delay before the next poll, from the latest pending
    /// response.
    ///
    /// `None` when the operation is not pending or the service sent no
    /// `Retry-After`.
    #[must_use]
    pub const fn retry_after(&self) -> Option<Duration> {
        match &self.state {
            State::Pending(task) => task.retry_after,
            State::Ready(_) | State::Finished | State::Done => None,
        }
    }

    /// Perform one polling step, returning the result once the operation
    /// completes and `None` while it is still running.
    ///
    /// A result read from a URI other than the Task Monitor can be left over
    /// from an earlier run, so callers should check request-specific data such
    /// as a nonce.
    ///
    /// # Errors
    ///
    /// Returns an error if a request fails, the operation completes without
    /// exposing its result, or an earlier poll already returned the result.
    /// After a request error the same step can be retried.
    pub async fn poll<B>(&mut self, task_service: &TaskService<B>) -> Result<Option<R>, Error<B>>
    where
        B: OperationResponseBmc,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        let bmc = task_service.bmc.as_ref();
        if matches!(self.state, State::Ready(_)) {
            let State::Ready(result) = mem::replace(&mut self.state, State::Done) else {
                return Err(Error::TaskAlreadyFinished);
            };
            return Ok(Some(result));
        }
        if matches!(self.state, State::Done) {
            return Err(Error::TaskAlreadyFinished);
        }

        if let State::Pending(task) = &self.state {
            let location = task.location.0.clone();
            match bmc
                .get_operation_response(&location)
                .await
                .map_err(Error::Bmc)?
            {
                ModificationResponse::Task(pending) => {
                    self.state = State::Pending(pending);
                    return Ok(None);
                }
                ModificationResponse::Entity(result) => {
                    self.state = State::Done;
                    return Ok(Some(result));
                }
                ModificationResponse::Empty => {
                    self.state = State::Finished;
                }
            }
        }

        self.poll_finished(bmc).await
    }

    async fn poll_finished<B>(&mut self, bmc: &B) -> Result<Option<R>, Error<B>>
    where
        B: OperationResponseBmc,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        let result = self.read_result(bmc).await;
        if !matches!(result, Err(Error::Bmc(_))) {
            self.state = State::Done;
        }
        result
    }

    async fn read_result<B>(&self, bmc: &B) -> Result<Option<R>, Error<B>>
    where
        B: OperationResponseBmc,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        let Some(result) = &self.compatibility_result else {
            return Err(Error::TaskResultUnavailable);
        };
        match bmc.get_operation_response(result).await {
            Ok(ModificationResponse::Entity(value)) => Ok(Some(value)),
            Ok(ModificationResponse::Task(_) | ModificationResponse::Empty) => {
                Err(Error::TaskResultUnavailable)
            }
            Err(error) => Err(Error::Bmc(error)),
        }
    }
}

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
/// let task_link = task_service.task_link(async_task)?;
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

    /// Create a task link from an asynchronous operation result.
    ///
    /// The task location must be a child of this service's Tasks collection,
    /// such as `/redfish/v1/TaskService/Tasks/{id}`. The returned link does not
    /// fetch the task until [`TaskLink::fetch`] is called.
    ///
    /// # Errors
    ///
    /// Returns error if the task location is not a child of this service's Tasks
    /// collection.
    pub fn task_link(&self, task: AsyncTask) -> Result<TaskLink<B>, Error<B>> {
        let Some(tasks) = self.data.tasks.as_ref() else {
            return Err(Error::TaskServiceTasksUnavailable);
        };

        let task_collection = tasks.odata_id();
        let task_location = task.location.0;
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
