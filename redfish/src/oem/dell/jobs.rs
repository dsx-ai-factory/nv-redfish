// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell configuration-job creation through the OEM Jobs endpoint advertised
//! by a Manager.

use serde::{Deserialize, Serialize};

use crate::core::{
    AsyncTask, Bmc, EntityTypeRef, ModificationResponse, NavProperty, ODataETag, ODataId,
};
use crate::{Error, NvBmc};

/// Minimal schema for the Dell OEM configuration Jobs link.
#[derive(Debug, Deserialize)]
pub(super) struct DellJobCollectionSchema {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
    #[serde(rename = "@odata.etag", default)]
    etag: Option<ODataETag>,
}

impl EntityTypeRef for DellJobCollectionSchema {
    fn odata_id(&self) -> &ODataId {
        &self.odata_id
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.etag.as_ref()
    }
}

#[derive(Debug, Deserialize)]
struct ConfigurationJobReference {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
}

impl EntityTypeRef for ConfigurationJobReference {
    fn odata_id(&self) -> &ODataId {
        &self.odata_id
    }

    fn etag(&self) -> Option<&ODataETag> {
        None
    }
}

#[derive(Serialize)]
struct CreateConfigurationJob<'a> {
    #[serde(rename = "TargetSettingsURI")]
    target_settings_uri: &'a ODataId,
}

/// Dell OEM configuration Jobs endpoint.
pub struct DellJobs<B: Bmc> {
    bmc: NvBmc<B>,
    collection_id: ODataId,
}

impl<B: Bmc> DellJobs<B> {
    pub(super) fn from_nav(
        bmc: &NvBmc<B>,
        collection: &NavProperty<DellJobCollectionSchema>,
    ) -> Self {
        Self::from_id(bmc, collection.id().clone())
    }

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
        self.bmc
            .as_ref()
            .create::<_, ConfigurationJobReference>(
                &self.collection_id,
                &CreateConfigurationJob {
                    target_settings_uri,
                },
            )
            .await
            .map(|response| match response {
                ModificationResponse::Entity(data) => {
                    // iDRAC reports a scheduled configuration job as 200 with
                    // a success envelope and the OEM job URI in Location. The
                    // HTTP transport preserves that URI as this minimal entity
                    // reference; expose it as asynchronous work to callers.
                    ModificationResponse::Task(AsyncTask {
                        location: data.odata_id.into(),
                        retry_after: None,
                    })
                }
                ModificationResponse::Task(task) => ModificationResponse::Task(task),
                ModificationResponse::Empty => ModificationResponse::Empty,
            })
            .map_err(Error::Bmc)
    }

    /// Advertised Dell jobs collection identifier.
    #[must_use]
    pub const fn odata_id(&self) -> &ODataId {
        &self.collection_id
    }
}
