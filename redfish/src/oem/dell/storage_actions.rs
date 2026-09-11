// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell actions advertised by a Storage resource.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::core::{Action, ActionError, Bmc, ModificationResponse};
use crate::schema::settings::ApplyTime;
use crate::schema::storage::OemActions as StorageOemActions;
use crate::{Error, NvBmc};

#[derive(Debug, Deserialize)]
struct DellStorageOemActions {
    #[serde(rename = "#DellStorage.ControllerDrivesDecommission")]
    controller_drives_decommission: Option<Action<DecommissionRequest, ()>>,
}

#[derive(Debug, Serialize)]
struct DecommissionRequest {
    #[serde(rename = "@Redfish.OperationApplyTime")]
    operation_apply_time: ApplyTime,
}

/// Dell actions advertised by a Storage resource.
pub struct DellStorageActions<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<DellStorageOemActions>,
}

impl<B: Bmc> DellStorageActions<B> {
    pub(crate) fn new(bmc: &NvBmc<B>, actions: &StorageOemActions) -> Result<Self, Error<B>> {
        DellStorageOemActions::deserialize(&actions.additional_properties)
            .map_err(Error::Json)
            .map(|data| Self {
                bmc: bmc.clone(),
                data: Arc::new(data),
            })
    }

    /// Decommission every drive attached to the controller.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ActionNotAvailable`] when the Storage resource does
    /// not advertise this action, or a BMC error if invocation fails.
    pub async fn decommission_controller_drives(
        &self,
        apply_time: ApplyTime,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let action = self
            .data
            .controller_drives_decommission
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        action
            .run(
                self.bmc.as_ref(),
                &DecommissionRequest {
                    operation_apply_time: apply_time,
                },
            )
            .await
            .map_err(Error::Bmc)
    }
}
