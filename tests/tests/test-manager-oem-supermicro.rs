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
//! Integration tests for Supermicro Manager OEM support.

use nv_redfish::manager::Manager;
use nv_redfish::oem::supermicro::Privilege;
use nv_redfish::oem::supermicro::ResetOption;
use nv_redfish::Error as RedfishError;
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
const SUPERMICRO_MANAGER_DATA_TYPE: &str = "#SmcManagerExtensions.v1_0_0.Manager";
const KCS_INTERFACE_DATA_TYPE: &str = "#KCSInterface.v1_1_0.KCSInterface";
const SYS_LOCKDOWN_DATA_TYPE: &str = "#SysLockdown.v1_0_0.SysLockdown";

#[test]
async fn supermicro_kcs_and_sys_lockdown_supported() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(
            &ids,
            Some(ids.kcs_interface_ref()),
            Some(ids.sys_lockdown_ref()),
        ),
    )
    .await?;

    let supermicro = manager.oem_supermicro()?.unwrap();
    bmc.expect(Expect::get(
        &ids.kcs_interface_id,
        kcs_interface_payload(&ids, "Administrator", "7f21b53f195494a7c2dad2008917b1d7"),
    ));
    let kcs = supermicro.kcs_interface().await?.unwrap();
    assert_eq!(kcs.privilege(), Some(Privilege::Administrator));

    bmc.expect(Expect::update(
        &ids.kcs_interface_id,
        json!({ "Privilege": "Callback" }),
        kcs_interface_payload(&ids, "Callback", "\"cb6b6a9b633e6f9e4051140515d96e6f\""),
    ));
    let ModificationResponse::Entity(kcs) = kcs.set_privilege(Privilege::Callback).await? else {
        return Err("expected updated KCS entity".into());
    };
    assert_eq!(kcs.privilege(), Some(Privilege::Callback));

    bmc.expect(Expect::get(
        &ids.sys_lockdown_id,
        sys_lockdown_payload(&ids, false, "30b691549156f2528aac46ed839cf7f6"),
    ));
    let lockdown = supermicro.sys_lockdown().await?.unwrap();
    assert_eq!(lockdown.sys_lockdown_enabled(), Some(false));

    bmc.expect(Expect::update(
        &ids.sys_lockdown_id,
        json!({ "SysLockdownEnabled": true }),
        sys_lockdown_payload(&ids, true, "\"6ddb3bc56adbae8094bcff1ad987c79c\""),
    ));
    let ModificationResponse::Entity(lockdown) = lockdown.set_enabled(true).await? else {
        return Err("expected updated SysLockdown entity".into());
    };
    assert_eq!(lockdown.sys_lockdown_enabled(), Some(true));

    bmc.expect(Expect::action(
        &ids.reset_target,
        json!({ "Option": "ClearConfig" }),
        json!(null),
    ));
    assert!(matches!(
        supermicro
            .reset_configuration(ResetOption::ClearConfig)
            .await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn supermicro_manager_without_kcs_still_supports_sys_lockdown(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, None, Some(ids.sys_lockdown_ref())),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.sys_lockdown_id,
        sys_lockdown_payload(&ids, false, "\"30b691549156f2528aac46ed839cf7f6\""),
    ));

    let supermicro = manager.oem_supermicro()?.unwrap();
    assert!(supermicro.kcs_interface().await?.is_none());

    let lockdown = supermicro.sys_lockdown().await?.unwrap();
    assert_eq!(lockdown.sys_lockdown_enabled(), Some(false));

    Ok(())
}

#[test]
async fn manager_reset_requires_advertisement() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let mut payload = manager_payload(
        &ids,
        Some(ids.kcs_interface_ref()),
        Some(ids.sys_lockdown_ref()),
    );
    payload["Actions"]["Oem"] = json!({});
    let manager = get_manager(bmc, &ids, payload).await?;
    let supermicro = manager.oem_supermicro()?.unwrap();

    assert!(matches!(
        supermicro
            .reset_configuration(ResetOption::ClearConfig)
            .await,
        Err(RedfishError::ActionNotAvailable)
    ));

    Ok(())
}

#[test]
async fn manager_without_supermicro_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload_without_supermicro(&ids)).await?;

    assert!(manager.oem_supermicro()?.is_none());

    Ok(())
}

#[test]
async fn manager_with_null_supermicro_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload_without_supermicro(&ids),
            &json!({ "Oem": { "Supermicro": null } }),
        ]),
    )
    .await?;

    assert!(manager.oem_supermicro()?.is_none());

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
    kcs_interface_id: String,
    sys_lockdown_id: String,
    reset_target: String,
}

impl Ids {
    fn kcs_interface_ref(&self) -> Value {
        json!({ ODATA_ID: &self.kcs_interface_id })
    }

    fn sys_lockdown_ref(&self) -> Value {
        json!({ ODATA_ID: &self.sys_lockdown_id })
    }
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    let manager_id = format!("{managers_id}/1");
    let kcs_interface_id = format!("{manager_id}/Oem/Supermicro/KCSInterface");
    let sys_lockdown_id = format!("{manager_id}/Oem/Supermicro/SysLockdown");
    let reset_target = format!("{manager_id}/Actions/Oem/SmcManagerConfig.Reset");
    Ids {
        root_id,
        managers_id,
        manager_id,
        kcs_interface_id,
        sys_lockdown_id,
        reset_target,
    }
}

fn manager_payload(ids: &Ids, kcs_interface: Option<Value>, sys_lockdown: Option<Value>) -> Value {
    let base = json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "ManagerType": "BMC",
        "Status": { "State": "Enabled" },
        "Actions": {
            "Oem": {
                "#SmcManagerConfig.Reset": {
                    "target": &ids.reset_target
                }
            }
        },
    });

    let mut supermicro = json!({
        ODATA_TYPE: SUPERMICRO_MANAGER_DATA_TYPE,
    });
    if let Some(v) = kcs_interface {
        supermicro["KCSInterface"] = v;
    }
    if let Some(v) = sys_lockdown {
        supermicro["SysLockdown"] = v;
    }

    let oem = json!({
        "Oem": {
            "Supermicro": supermicro
        }
    });
    json_merge([&base, &oem])
}

fn manager_payload_without_supermicro(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "ManagerType": "BMC",
        "Status": { "State": "Enabled" },
        "Oem": {}
    })
}

fn kcs_interface_payload(ids: &Ids, privilege: &str, etag: &str) -> Value {
    json!({
        ODATA_ID: &ids.kcs_interface_id,
        ODATA_TYPE: KCS_INTERFACE_DATA_TYPE,
        "Id": "KCSInterface",
        "Name": "KCS Interface",
        "Privilege": privilege,
        "@odata.etag": etag
    })
}

fn sys_lockdown_payload(ids: &Ids, enabled: bool, etag: &str) -> Value {
    json!({
        ODATA_ID: &ids.sys_lockdown_id,
        ODATA_TYPE: SYS_LOCKDOWN_DATA_TYPE,
        "Id": "SysLockdown",
        "Name": "SysLockdown",
        "SysLockdownEnabled": enabled,
        "@odata.etag": etag
    })
}
