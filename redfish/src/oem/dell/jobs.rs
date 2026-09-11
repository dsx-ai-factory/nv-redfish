// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell job creation through a standard Redfish Jobs collection.

use std::marker::PhantomData;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::{Bmc, EntityTypeRef, ModificationResponse, ODataETag, ODataId};
use crate::{Error, NvBmc};

#[cfg(feature = "managers")]
use crate::core::NavProperty;

/// Minimal schema for the legacy Dell OEM Jobs collection link.
#[cfg(feature = "managers")]
#[derive(Debug, Deserialize)]
pub(super) struct DellJobCollectionSchema {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
    #[serde(rename = "@odata.etag", default)]
    etag: Option<ODataETag>,
}

#[cfg(feature = "managers")]
impl EntityTypeRef for DellJobCollectionSchema {
    fn odata_id(&self) -> &ODataId {
        &self.odata_id
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.etag.as_ref()
    }
}

/// Schema for a Dell job.
#[derive(Debug, Deserialize)]
pub struct DellJobSchema {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
    #[serde(rename = "@odata.etag", default)]
    etag: Option<ODataETag>,
    #[serde(rename = "JobState", default)]
    job_state: Option<String>,
}

impl EntityTypeRef for DellJobSchema {
    fn odata_id(&self) -> &ODataId {
        &self.odata_id
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.etag.as_ref()
    }
}

#[derive(Serialize)]
struct CreateConfigurationJob<'a> {
    #[serde(rename = "TargetSettingsURI")]
    target_settings_uri: &'a ODataId,
}

/// Dell job collection handle.
pub struct DellJobs<B: Bmc> {
    bmc: NvBmc<B>,
    collection_id: ODataId,
}

impl<B: Bmc> DellJobs<B> {
    #[cfg(feature = "managers")]
    pub(super) fn from_legacy_nav(
        bmc: &NvBmc<B>,
        collection: &NavProperty<DellJobCollectionSchema>,
    ) -> Self {
        Self::from_id(bmc, collection.id().clone())
    }

    pub(crate) fn from_id(bmc: &NvBmc<B>, collection_id: ODataId) -> Self {
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
    ) -> Result<ModificationResponse<DellJob<B>>, Error<B>> {
        self.bmc
            .as_ref()
            .create::<_, DellJobSchema>(
                &self.collection_id,
                &CreateConfigurationJob {
                    target_settings_uri,
                },
            )
            .await
            .map(|response| {
                response.map_entity(|data| DellJob {
                    data: Arc::new(data),
                    _marker: PhantomData,
                })
            })
            .map_err(Error::Bmc)
    }

    /// Advertised Dell jobs collection identifier.
    #[must_use]
    pub const fn odata_id(&self) -> &ODataId {
        &self.collection_id
    }
}

/// Dell job handle.
pub struct DellJob<B: Bmc> {
    data: Arc<DellJobSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> DellJob<B> {
    /// Dell job state exactly as reported by the BMC.
    #[must_use]
    pub fn state(&self) -> Option<&str> {
        self.data.job_state.as_deref()
    }

    /// Get the raw Dell job schema.
    #[must_use]
    pub fn raw(&self) -> Arc<DellJobSchema> {
        self.data.clone()
    }
}
