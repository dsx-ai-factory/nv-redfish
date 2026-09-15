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

//! Dell resources advertised from a Manager's OEM links.

use std::sync::Arc;

use serde::Deserialize as _;

use crate::core::Bmc;
#[cfg(feature = "oem-dell-attributes")]
use crate::core::EntityTypeRef as _;
#[cfg(feature = "oem-dell-attributes")]
use crate::oem::dell::attributes::DellAttributes;
#[cfg(feature = "job-service")]
use crate::oem::dell::job_service::DellJobService;
#[cfg(feature = "job-service")]
use crate::oem::dell::jobs::DellJobs;
#[cfg(feature = "job-service")]
use crate::oem::dell::schema::dell_manager::DellManager as DellManagerSchema;
use crate::oem::dell::schema::dell_manager::DellManagerLinks as DellManagerLinksSchema;
use crate::oem::oem_value;
use crate::schema::manager::Manager as ManagerSchema;
use crate::{Error, NvBmc};

/// Dell resources linked by a Manager.
pub struct DellManager<B: Bmc> {
    bmc: NvBmc<B>,
    #[cfg(feature = "oem-dell-attributes")]
    manager_id: String,
    links: Option<Arc<DellManagerLinksSchema>>,
    #[cfg(feature = "job-service")]
    resources: Option<Arc<DellManagerSchema>>,
}

impl<B: Bmc> DellManager<B> {
    /// Parse Dell Manager resources from both supported OEM locations.
    pub(crate) fn new(bmc: &NvBmc<B>, manager: &ManagerSchema) -> Result<Option<Self>, Error<B>> {
        let links = manager
            .links
            .as_ref()
            .and_then(|links| links.base.oem.as_ref())
            .and_then(|oem| oem_value(oem, "Dell"))
            .map(DellManagerLinksSchema::deserialize)
            .transpose()
            .map_err(Error::Json)?
            .map(Arc::new);
        #[cfg(feature = "job-service")]
        let resources = manager
            .base
            .base
            .oem
            .as_ref()
            .and_then(|oem| oem_value(oem, "Dell"))
            .map(DellManagerSchema::deserialize)
            .transpose()
            .map_err(Error::Json)?
            .map(Arc::new);

        let links_are_empty = links.as_ref().is_none_or(|links| {
            links.dell_attributes.as_ref().is_none_or(Vec::is_empty)
                && links.dell_job_service.is_none()
                && links.jobs.is_none()
        });
        #[cfg(feature = "job-service")]
        let resources_are_empty = resources
            .as_ref()
            .is_none_or(|resources| resources.jobs.is_none());
        #[cfg(not(feature = "job-service"))]
        let resources_are_empty = true;
        if links_are_empty && resources_are_empty {
            return Ok(None);
        }

        Ok(Some(Self {
            bmc: bmc.clone(),
            #[cfg(feature = "oem-dell-attributes")]
            manager_id: manager.base.id.clone(),
            links,
            #[cfg(feature = "job-service")]
            resources,
        }))
    }

    /// Fetch the Dell attributes resource corresponding to this Manager.
    ///
    /// # Errors
    ///
    /// Returns an error if the advertised resource cannot be fetched.
    #[cfg(feature = "oem-dell-attributes")]
    pub async fn manager_attributes(&self) -> Result<Option<DellAttributes<B>>, Error<B>> {
        let Some(nav) = self
            .links
            .as_ref()
            .and_then(|links| links.dell_attributes.as_ref())
            .and_then(|attributes| {
                attributes
                    .iter()
                    .find(|nav| nav.odata_id().last_segment() == Some(self.manager_id.as_str()))
            })
        else {
            return Ok(None);
        };
        DellAttributes::new_advertised(&self.bmc, nav)
            .await
            .map(Some)
    }

    /// Fetch the advertised Dell job service.
    ///
    /// # Errors
    ///
    /// Returns an error if the advertised resource cannot be fetched.
    #[cfg(feature = "job-service")]
    pub async fn job_service(&self) -> Result<Option<DellJobService<B>>, Error<B>> {
        match self
            .links
            .as_ref()
            .and_then(|links| links.dell_job_service.as_ref())
        {
            Some(nav) => DellJobService::new(&self.bmc, nav).await.map(Some),
            None => Ok(None),
        }
    }

    /// Get the Dell OEM configuration jobs endpoint.
    #[cfg(feature = "job-service")]
    #[must_use]
    pub fn configuration_jobs(&self) -> Option<DellJobs<B>> {
        // iDRAC9 advertises Jobs in Links.Oem.Dell, while iDRAC10 advertises
        // it in top-level Oem.Dell. Prefer the advertised Links value if both
        // layouts are present.
        self.links
            .as_ref()
            .and_then(|links| links.jobs.as_ref())
            .or_else(|| {
                self.resources
                    .as_ref()
                    .and_then(|resources| resources.jobs.as_ref())
            })
            .map(|jobs| DellJobs::from_nav(&self.bmc, jobs))
    }
}
