// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell RAID volume creation through a standard Redfish Volumes collection.

use serde::Serialize;

use crate::computer_system::storage::Volume;
use crate::core::{Bmc, ModificationResponse, ODataId};
use crate::schema::volume::{RaidType, Volume as VolumeSchema};
use crate::{Error, NvBmc};

#[derive(Debug, Serialize)]
struct DriveReference {
    #[serde(rename = "@odata.id")]
    odata_id: ODataId,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "PascalCase")]
struct VolumeLinks {
    drives: Vec<DriveReference>,
}

/// Typed Dell request for creating a RAID volume.
#[derive(Debug, Serialize)]
pub struct DellVolumeCreate {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "RAIDType")]
    raid_type: RaidType,
    #[serde(rename = "Links")]
    links: VolumeLinks,
}

impl DellVolumeCreate {
    /// Build a Dell RAID volume request from caller-selected policy values.
    #[must_use]
    pub fn new(name: String, raid_type: RaidType, drives: Vec<ODataId>) -> Self {
        Self {
            name,
            raid_type,
            links: VolumeLinks {
                drives: drives
                    .into_iter()
                    .map(|odata_id| DriveReference { odata_id })
                    .collect(),
            },
        }
    }
}

/// Dell operations on a standard Redfish Volumes collection.
pub struct DellVolumes<B: Bmc> {
    bmc: NvBmc<B>,
    collection_id: ODataId,
}

impl<B: Bmc> DellVolumes<B> {
    pub(crate) fn new(bmc: &NvBmc<B>, collection_id: ODataId) -> Self {
        Self {
            bmc: bmc.clone(),
            collection_id,
        }
    }

    /// Create a Dell RAID volume.
    ///
    /// # Errors
    ///
    /// Returns an error if the BMC rejects the request.
    pub async fn create(
        &self,
        request: &DellVolumeCreate,
    ) -> Result<ModificationResponse<Volume<B>>, Error<B>> {
        self.bmc
            .as_ref()
            .create::<_, VolumeSchema>(&self.collection_id, request)
            .await
            .map(|response| response.map_entity(Volume::from_data))
            .map_err(Error::Bmc)
    }
}
