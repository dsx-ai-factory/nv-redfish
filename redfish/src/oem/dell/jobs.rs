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

//! Dell configuration jobs through the OEM Jobs endpoint advertised by a
//! Manager: creating them and linking to them for status.

use crate::core::{AsyncTask, Bmc, EntityTypeRef as _, ModificationResponse, NavProperty, ODataId};
use crate::entity_link::EntityLink;
use crate::oem::dell::schema::dell_job::{DellJob, DellJobCreate};
#[cfg(feature = "managers")]
use crate::oem::dell::schema::dell_job_collection::DellJobCollection;
use crate::{Error, NvBmc};

/// Link to a Dell job returned by an asynchronous operation.
pub type DellJobLink<B> = EntityLink<B, DellJob>;

/// Dell OEM configuration Jobs endpoint.
pub struct DellJobs<B: Bmc> {
    bmc: NvBmc<B>,
    collection_id: ODataId,
}

impl<B: Bmc> DellJobs<B> {
    /// Create a handle from an advertised Dell Jobs collection.
    #[cfg(feature = "managers")]
    pub(super) fn from_nav(bmc: &NvBmc<B>, collection: &NavProperty<DellJobCollection>) -> Self {
        Self::from_id(bmc, collection.id().clone())
    }

    /// Create a handle from a Dell Jobs collection identifier.
    #[cfg(feature = "managers")]
    fn from_id(bmc: &NvBmc<B>, collection_id: ODataId) -> Self {
        Self {
            bmc: bmc.clone(),
            collection_id,
        }
    }

    /// Create a configuration job for an advertised settings resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the BMC rejects the request.
    pub async fn create_configuration_job(
        &self,
        target_settings_uri: &ODataId,
    ) -> Result<ModificationResponse<()>, Error<B>> {
        let create = DellJobCreate::builder(target_settings_uri.to_string()).build();
        self.bmc
            .as_ref()
            .create::<_, NavProperty<DellJob>>(&self.collection_id, &create)
            .await
            .map(|response| match response {
                ModificationResponse::Entity(job) => {
                    // iDRAC reports a scheduled configuration job as 200 with
                    // a success envelope and the OEM job URI in Location. The
                    // HTTP transport preserves that URI as this minimal entity
                    // reference; expose it as asynchronous work to callers.
                    ModificationResponse::Task(AsyncTask {
                        location: job.odata_id().clone().into(),
                        retry_after: None,
                        task: None,
                    })
                }
                ModificationResponse::Task(task) => ModificationResponse::Task(task),
                ModificationResponse::Message(message) => ModificationResponse::Message(message),
                ModificationResponse::Empty => ModificationResponse::Empty,
            })
            .map_err(Error::from)
    }

    /// Create a job link from an asynchronous operation result, such as the
    /// one [`Self::create_configuration_job`] returns.
    ///
    /// The job location must be a child of this Jobs collection, such as
    /// `/redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/Jobs/{id}`. The
    /// returned link does not fetch the job until [`DellJobLink::fetch`] is
    /// called.
    ///
    /// # Errors
    ///
    /// Returns an error if the job location is not a child of this Jobs
    /// collection.
    pub fn job_link(&self, job: AsyncTask) -> Result<DellJobLink<B>, Error<B>> {
        let job_location = job.location.0;
        if self.collection_id == job_location || !self.collection_id.is_path_prefix(&job_location) {
            return Err(Error::JobLocationNotInJobs {
                job_location,
                job_collection: self.collection_id.clone(),
            });
        }
        Ok(DellJobLink::new(
            &self.bmc,
            NavProperty::new_reference(job_location),
        ))
    }

    /// Advertised Dell jobs collection identifier.
    #[must_use]
    pub const fn odata_id(&self) -> &ODataId {
        &self.collection_id
    }
}
