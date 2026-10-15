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

//! Standard Redfish JobService resources.

use std::sync::Arc;

use crate::core::{AsyncTask, Bmc, NavProperty, ODataId};
use crate::entity_link::EntityLink;
use crate::schema::job::Job as JobSchema;
use crate::schema::job_collection::JobCollection as JobCollectionSchema;
use crate::schema::job_service::JobService as JobServiceSchema;
use crate::{Error, NvBmc, ServiceRoot};

/// Link to a standard Redfish Job resource.
pub type JobLink<B> = EntityLink<B, JobSchema>;

/// Standard Redfish JobService.
pub struct JobService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<JobServiceSchema>,
}

impl<B: Bmc> JobService<B> {
    /// Fetch the JobService advertised by the service root.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(service_ref) = &root.root.job_service else {
            return Ok(None);
        };
        service_ref
            .get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| {
                Some(Self {
                    bmc: bmc.clone(),
                    data,
                })
            })
    }

    /// Get the Jobs collection advertised by this service.
    #[must_use]
    pub fn jobs(&self) -> Option<JobCollection<B>> {
        self.data
            .jobs
            .as_ref()
            .map(|jobs| JobCollection::new(&self.bmc, jobs))
    }

    /// Get the raw JobService schema.
    #[must_use]
    pub fn raw(&self) -> Arc<JobServiceSchema> {
        self.data.clone()
    }
}

/// Standard Redfish Job collection.
pub struct JobCollection<B: Bmc> {
    bmc: NvBmc<B>,
    id: ODataId,
}

impl<B: Bmc> JobCollection<B> {
    /// Create a collection handle from an advertised navigation property.
    fn new(bmc: &NvBmc<B>, collection: &NavProperty<JobCollectionSchema>) -> Self {
        Self {
            bmc: bmc.clone(),
            id: collection.id().clone(),
        }
    }

    /// Advertised collection identifier.
    #[must_use]
    pub const fn odata_id(&self) -> &ODataId {
        &self.id
    }

    /// Create a job link from an asynchronous operation result.
    ///
    /// The job location must be a child of this Jobs collection, such as
    /// `/redfish/v1/JobService/Jobs/{id}`. The returned link does not fetch
    /// the job until [`JobLink::fetch`] is called.
    ///
    /// # Errors
    ///
    /// Returns an error if the job location is not a child of this Jobs
    /// collection.
    pub fn job_link(&self, job: AsyncTask) -> Result<JobLink<B>, Error<B>> {
        let job_location = job.location.0;
        if self.id == job_location || !self.id.is_path_prefix(&job_location) {
            return Err(Error::JobLocationNotInJobs {
                job_location,
                job_collection: self.id.clone(),
            });
        }
        Ok(EntityLink::new(
            &self.bmc,
            NavProperty::new_reference(job_location),
        ))
    }

    /// Fetch links to every Job currently in the collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection cannot be fetched.
    pub async fn member_links(&self) -> Result<Vec<JobLink<B>>, Error<B>> {
        let collection = NavProperty::<JobCollectionSchema>::new_reference(self.id.clone())
            .get(self.bmc.as_ref())
            .await
            .map_err(Error::from)?;
        Ok(collection
            .members
            .iter()
            .map(|job| EntityLink::new(&self.bmc, NavProperty::new_reference(job.id().clone())))
            .collect())
    }
}
