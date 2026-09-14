// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell resources advertised from a Manager's OEM links.

use std::sync::Arc;

use serde::Deserialize;

use crate::core::{Bmc, EntityTypeRef as _, NavProperty};
use crate::oem::dell::attributes::DellAttributes;
use crate::oem::dell::job_service::DellJobService;
use crate::oem::dell::jobs::{DellJobCollectionSchema, DellJobs};
use crate::oem::dell::schema::dell_attributes::DellAttributes as DellAttributesSchema;
use crate::oem::dell::schema::dell_job_service::DellJobService as DellJobServiceSchema;
use crate::oem::oem_value;
use crate::schema::manager::Manager as ManagerSchema;
use crate::{Error, NvBmc};

#[derive(Debug, Deserialize)]
struct DellManagerLinks {
    #[serde(rename = "DellAttributes", default)]
    attributes: Vec<NavProperty<DellAttributesSchema>>,
    #[serde(rename = "DellJobService", default)]
    job_service: Option<NavProperty<DellJobServiceSchema>>,
    #[serde(rename = "Jobs", default)]
    jobs: Option<NavProperty<DellJobCollectionSchema>>,
}

#[derive(Debug, Default, Deserialize)]
struct DellManagerResources {
    #[serde(rename = "Jobs", default)]
    jobs: Option<NavProperty<DellJobCollectionSchema>>,
}

/// Dell resources linked by a Manager.
pub struct DellManager<B: Bmc> {
    bmc: NvBmc<B>,
    manager_id: String,
    links: Arc<DellManagerLinks>,
}

impl<B: Bmc> DellManager<B> {
    pub(crate) fn new(bmc: &NvBmc<B>, manager: &ManagerSchema) -> Result<Option<Self>, Error<B>> {
        let Some(oem) = manager
            .links
            .as_ref()
            .and_then(|links| links.base.oem.as_ref())
        else {
            return Ok(None);
        };
        let Some(dell) = oem_value(oem, "Dell") else {
            return Ok(None);
        };
        let mut links = DellManagerLinks::deserialize(dell).map_err(Error::Json)?;
        let resources = manager
            .base
            .base
            .oem
            .as_ref()
            .and_then(|oem| oem_value(oem, "Dell"))
            .map(DellManagerResources::deserialize)
            .transpose()
            .map_err(Error::Json)?
            .unwrap_or_default();
        links.jobs = links.jobs.or(resources.jobs);
        if links.attributes.is_empty() && links.job_service.is_none() && links.jobs.is_none() {
            return Ok(None);
        }
        Ok(Some(Self {
            bmc: bmc.clone(),
            manager_id: manager.base.id.clone(),
            links: Arc::new(links),
        }))
    }

    /// Fetch the Dell attributes resource corresponding to this Manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the advertised resource cannot be fetched.
    pub async fn manager_attributes(&self) -> Result<Option<DellAttributes<B>>, Error<B>> {
        let Some(nav) = self
            .links
            .attributes
            .iter()
            .find(|nav| nav.odata_id().last_segment() == Some(self.manager_id.as_str()))
        else {
            return Ok(None);
        };
        DellAttributes::new(&self.bmc, nav).await.map(Some)
    }

    /// Fetch the advertised Dell job service.
    ///
    /// # Errors
    ///
    /// Returns an error if the advertised resource cannot be fetched.
    pub async fn job_service(&self) -> Result<Option<DellJobService<B>>, Error<B>> {
        match &self.links.job_service {
            Some(nav) => DellJobService::new(&self.bmc, nav).await.map(Some),
            None => Ok(None),
        }
    }

    /// Get the Dell OEM configuration jobs endpoint.
    #[must_use]
    pub fn configuration_jobs(&self) -> Option<DellJobs<B>> {
        self.links
            .jobs
            .as_ref()
            .map(|jobs| DellJobs::from_nav(&self.bmc, jobs))
    }
}
