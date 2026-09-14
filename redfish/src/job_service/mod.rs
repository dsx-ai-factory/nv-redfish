// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Standard Redfish JobService resources.

use std::sync::Arc;

use crate::core::{Bmc, NavProperty, ODataId};
use crate::entity_link::EntityLink;
use crate::schema::job::Job as JobSchema;
use crate::schema::job_collection::JobCollection as JobCollectionSchema;
use crate::schema::job_service::JobService as JobServiceSchema;
use crate::{Error, NvBmc, Resource, ResourceSchema, ServiceRoot};

/// Link to a standard Redfish Job resource.
pub type JobLink<B> = EntityLink<B, JobSchema>;

/// Standard Redfish JobService.
pub struct JobService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<JobServiceSchema>,
}

impl<B: Bmc> JobService<B> {
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
            .map_err(Error::Bmc)
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

impl<B: Bmc> Resource for JobService<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}

/// Standard Redfish Job collection.
pub struct JobCollection<B: Bmc> {
    bmc: NvBmc<B>,
    id: ODataId,
}

impl<B: Bmc> JobCollection<B> {
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

    /// Fetch links to every Job currently in the collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the collection cannot be fetched.
    pub async fn members(&self) -> Result<Vec<JobLink<B>>, Error<B>> {
        let collection = NavProperty::<JobCollectionSchema>::new_reference(self.id.clone())
            .get(self.bmc.as_ref())
            .await
            .map_err(Error::Bmc)?;
        Ok(collection
            .members
            .iter()
            .map(|job| EntityLink::new(&self.bmc, NavProperty::new_reference(job.id().clone())))
            .collect())
    }
}
