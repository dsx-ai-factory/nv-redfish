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
//! Integration tests for AMI Manager OEM ConfigBMC support.

use nv_redfish::manager::Manager;
use nv_redfish::oem::ami::config_bmc::ConfigBmcUpdate;
use nv_redfish::oem::ami::config_bmc::LockdownBiosSettingsChangeState;
use nv_redfish::oem::ami::config_bmc::LockdownBiosUpgradeDowngradeState;
use nv_redfish::oem::ami::config_bmc::LockoutBiosVariableWriteMode;
use nv_redfish::oem::ami::config_bmc::LockoutHostControlState;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const MANAGER_COLLECTION_DATA_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_DATA_TYPE: &str = "#Manager.v1_16_0.Manager";
const AMI_MANAGER_OEM_DATA_TYPE: &str = "#AMIManager.v1_0_0.AMIManager";

#[test]
async fn manager_oem_ami_config_bmc_supported() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, true, true, ami_marker_payload()),
    )
    .await?;

    bmc.expect(Expect::expand(&ids.config_bmc_id, config_bmc_payload(&ids)));
    let config = manager
        .oem_ami_config_bmc()
        .await?
        .expect("AMI ConfigBMC must be available");
    let raw = config.raw();

    assert_eq!(
        raw.lockout_host_control,
        Some(LockoutHostControlState::Disable)
    );
    assert_eq!(
        raw.lockout_bios_variable_write_mode,
        Some(LockoutBiosVariableWriteMode::Disable)
    );
    assert_eq!(
        raw.lockdown_bios_settings_change,
        Some(LockdownBiosSettingsChangeState::Disable)
    );
    assert_eq!(
        raw.lockdown_bios_upgrade_downgrade,
        Some(LockdownBiosUpgradeDowngradeState::Disable)
    );
    let update = ConfigBmcUpdate::builder()
        .with_lockout_host_control(LockoutHostControlState::Enable)
        .with_lockout_bios_variable_write_mode(LockoutBiosVariableWriteMode::Enable)
        .with_lockdown_bios_settings_change(LockdownBiosSettingsChangeState::Enable)
        .with_lockdown_bios_upgrade_downgrade(LockdownBiosUpgradeDowngradeState::Enable)
        .build();
    let request = json!({
        "LockoutHostControl": "Enable",
        "LockoutBiosVariableWriteMode": "Enable",
        "LockdownBiosSettingsChange": "Enable",
        "LockdownBiosUpgradeDowngrade": "Enable"
    });
    bmc.expect(Expect::create(
        &ids.config_bmc_id,
        &request,
        json!({ "Result": "Success" }),
    ));
    assert!(matches!(
        config.apply(&update).await?,
        ModificationResponse::Entity(())
    ));
    bmc.expect(Expect::create_empty(&ids.config_bmc_id, &request));
    assert!(matches!(
        config.apply(&update).await?,
        ModificationResponse::Empty
    ));

    Ok(())
}

#[test]
async fn manager_without_ami_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, false, false, json!({})),
    )
    .await?;

    assert!(manager.oem_ami_config_bmc().await?.is_none());

    Ok(())
}

#[test]
async fn manager_with_null_ami_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    //
    // The ConfigBMC link is present, and no GET for it is expected:
    // the null must gate the lookup, not just the return value.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, true, true, json!(null)),
    )
    .await?;

    assert!(manager.oem_ami_config_bmc().await?.is_none());

    Ok(())
}

#[test]
async fn manager_ami_without_config_bmc_link_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, true, false, ami_marker_payload()),
    )
    .await?;

    assert!(manager.oem_ami_config_bmc().await?.is_none());

    Ok(())
}

async fn get_manager(
    bmc: Arc<Bmc>,
    ids: &Ids,
    manager: Value,
) -> Result<Manager<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [manager]
        }),
    ));

    let collection = root.managers().await?.unwrap();
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single manager must exist"))
}

async fn expect_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json!({
            ODATA_ID: &ids.root_id,
            ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "Managers": { ODATA_ID: &ids.managers_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct Ids {
    root_id: ODataId,
    managers_id: String,
    manager_id: String,
    config_bmc_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    let manager_id = format!("{managers_id}/Self");
    let config_bmc_id = format!("{manager_id}/Oem/ConfigBMC");
    Ids {
        root_id,
        managers_id,
        manager_id,
        config_bmc_id,
    }
}

fn ami_marker_payload() -> Value {
    json!({
        ODATA_TYPE: AMI_MANAGER_OEM_DATA_TYPE
    })
}

fn manager_payload(
    ids: &Ids,
    include_ami: bool,
    include_config_bmc_link: bool,
    ami_payload: Value,
) -> Value {
    let base = json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "Self",
        "Name": "Manager",
        "ManagerType": "BMC",
        "Status": { "State": "Enabled" },
    });

    let mut oem = json!({});
    if include_ami {
        oem["Ami"] = ami_payload;
    }
    if include_config_bmc_link {
        oem["ConfigBMC"] = json!(&ids.config_bmc_id);
    }

    let payload = json!({
        "Oem": oem
    });
    json_merge([&base, &payload])
}

fn config_bmc_payload(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.config_bmc_id,
        "LockoutHostControl": "Disable",
        "LockoutBiosVariableWriteMode": "Disable",
        "LockdownBiosSettingsChange": "Disable",
        "LockdownBiosUpgradeDowngrade": "Disable",
    })
}
