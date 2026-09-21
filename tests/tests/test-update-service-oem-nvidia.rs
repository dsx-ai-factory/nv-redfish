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

//! Integration tests for NVIDIA UpdateService OEM actions.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::update_service::UpdateService;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;

const UPDATE_SERVICE_ID: &str = "/redfish/v1/UpdateService";
const CLEAR_NVRAM_TARGET: &str =
    "/redfish/v1/UpdateService/Actions/Oem/NvidiaUpdateService.ClearNVRAM";
const COMMIT_IMAGE_TARGET: &str =
    "/redfish/v1/UpdateService/Actions/Oem/NvidiaUpdateService.CommitImage";
const HOST_BIOS_TARGET: &str = "/redfish/v1/UpdateService/FirmwareInventory/HostBIOS_0";
const BMC_TARGET: &str = "/redfish/v1/UpdateService/FirmwareInventory/FW_BMC_0";

#[tokio::test]
async fn nvidia_update_actions_use_advertised_targets() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let update_service = update_service(
        bmc.clone(),
        json!({
            "Actions": {
                "Oem": {
                    "#NvidiaUpdateService.ClearNVRAM": {
                        "target": CLEAR_NVRAM_TARGET
                    },
                    "#NvidiaUpdateService.CommitImage": {
                        "target": COMMIT_IMAGE_TARGET
                    },
                    "Nvidia": {
                        "#NvidiaUpdateService.PublicKeyExchange": {
                            "target": "/redfish/v1/UpdateService/Actions/Oem/NvidiaUpdateService.PublicKeyExchange"
                        }
                    }
                }
            }
        }),
    )
    .await?;
    let actions = update_service
        .oem_nvidia_actions()?
        .expect("NVIDIA actions are advertised");

    bmc.expect(Expect::action(
        CLEAR_NVRAM_TARGET,
        json!({ "Targets": [HOST_BIOS_TARGET] }),
        json!(null),
    ));
    assert!(matches!(
        actions
            .clear_nvram(vec![HOST_BIOS_TARGET.to_string()])
            .await?,
        ModificationResponse::Entity(())
    ));

    bmc.expect(Expect::action(
        COMMIT_IMAGE_TARGET,
        json!({ "Targets": [BMC_TARGET] }),
        json!(null),
    ));
    assert!(matches!(
        actions
            .commit_image(Some(vec![BMC_TARGET.to_string()]))
            .await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[tokio::test]
async fn nvidia_commit_image_can_omit_targets() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let update_service = update_service(
        bmc.clone(),
        json!({
            "Actions": {
                "Oem": {
                    "#NvidiaUpdateService.CommitImage": {
                        "target": COMMIT_IMAGE_TARGET
                    }
                }
            }
        }),
    )
    .await?;
    let actions = update_service
        .oem_nvidia_actions()?
        .expect("NVIDIA actions are advertised");

    bmc.expect(Expect::action(COMMIT_IMAGE_TARGET, json!({}), json!(null)));
    assert!(matches!(
        actions.commit_image(None).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[tokio::test]
async fn nvidia_update_actions_must_be_advertised() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let update_service = update_service(
        bmc,
        json!({
            "Actions": {
                "Oem": {}
            }
        }),
    )
    .await?;
    let actions = update_service
        .oem_nvidia_actions()?
        .expect("OEM actions object is advertised");

    assert!(matches!(
        actions
            .clear_nvram(vec![HOST_BIOS_TARGET.to_string()])
            .await,
        Err(Error::ActionNotAvailable)
    ));
    assert!(matches!(
        actions.commit_image(None).await,
        Err(Error::ActionNotAvailable)
    ));

    Ok(())
}

#[tokio::test]
async fn nvidia_update_actions_return_none_without_oem_actions() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let update_service = update_service(bmc, json!({})).await?;

    assert!(update_service.oem_nvidia_actions()?.is_none());

    Ok(())
}

async fn update_service(
    bmc: Arc<Bmc>,
    fields: Value,
) -> Result<UpdateService<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
            "Id": "RootService",
            "Name": "RootService",
            "UpdateService": {
                ODATA_ID: UPDATE_SERVICE_ID
            },
            "Links": {
                "Sessions": {
                    ODATA_ID: "/redfish/v1/SessionService/Sessions"
                }
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;

    let base = json!({
        ODATA_ID: UPDATE_SERVICE_ID,
        ODATA_TYPE: "#UpdateService.v1_11_0.UpdateService",
        "Id": "UpdateService",
        "Name": "UpdateService"
    });
    bmc.expect(Expect::get(
        UPDATE_SERVICE_ID,
        nv_redfish_tests::json_merge([&base, &fields]),
    ));

    root.update_service()
        .await?
        .ok_or_else(|| "expected UpdateService".into())
}
