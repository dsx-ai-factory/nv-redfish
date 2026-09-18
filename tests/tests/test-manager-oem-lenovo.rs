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
//! Integration tests for Lenovo Manager OEM support.

use nv_redfish::manager::Manager;
use nv_redfish::oem::lenovo::manager::KcsState;
use nv_redfish::oem::lenovo::security_service::FwRollbackState;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::assert_empty;
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
const SECURITY_SERVICE_DATA_TYPE: &str = "#LenovoSecurityService.v1_0_0.LenovoSecurityService";

#[test]
async fn lenovo_kcs_enabled_string_disabled_maps_state() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!("Disabled")), true),
    )
    .await?;

    let lenovo = manager.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.kcs_enabled(), Some(KcsState::Disabled));

    Ok(())
}

#[test]
async fn lenovo_kcs_enabled_boolean_true_maps_state() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!(true)), true),
    )
    .await?;

    let lenovo = manager.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.kcs_enabled(), Some(KcsState::Enabled));

    Ok(())
}

#[test]
async fn lenovo_kcs_update_preserves_string_representation() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!("Disabled")), true),
    )
    .await?;
    let request = json!({ "Oem": { "Lenovo": { "KCSEnabled": "Enabled" } } });
    bmc.expect(Expect::update(
        &ids.manager_id,
        &request,
        manager_payload(&ids, Some(json!("Enabled")), true),
    ));

    let lenovo = manager.oem_lenovo()?.ok_or("Lenovo OEM data missing")?;
    let ModificationResponse::Entity(updated) = lenovo
        .set_kcs_enabled(true)
        .await?
        .ok_or("KCS representation missing")?
    else {
        return Err("expected updated manager".into());
    };
    assert_eq!(
        updated.oem_lenovo()?.and_then(|oem| oem.kcs_enabled()),
        Some(KcsState::Enabled)
    );
    let updated_lenovo = updated
        .oem_lenovo()?
        .ok_or("updated Lenovo OEM data missing")?;
    bmc.expect(Expect::update_empty(&ids.manager_id, &request));
    assert_empty(
        updated_lenovo
            .set_kcs_enabled(true)
            .await?
            .ok_or("KCS representation missing")?,
    );
    Ok(())
}

#[test]
async fn lenovo_kcs_update_preserves_boolean_representation() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!(true)), true),
    )
    .await?;
    bmc.expect(Expect::update(
        &ids.manager_id,
        json!({ "Oem": { "Lenovo": { "KCSEnabled": false } } }),
        manager_payload(&ids, Some(json!(false)), true),
    ));

    let lenovo = manager.oem_lenovo()?.ok_or("Lenovo OEM data missing")?;
    let ModificationResponse::Entity(updated) = lenovo
        .set_kcs_enabled(false)
        .await?
        .ok_or("KCS representation missing")?
    else {
        return Err("expected updated manager".into());
    };
    assert_eq!(
        updated.oem_lenovo()?.and_then(|oem| oem.kcs_enabled()),
        Some(KcsState::Disabled)
    );
    Ok(())
}

#[test]
async fn lenovo_kcs_update_requires_known_wire_representation() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let missing = get_manager(bmc.clone(), &ids, manager_payload(&ids, None, false)).await?;
    assert!(missing
        .oem_lenovo()?
        .ok_or("Lenovo OEM data missing")?
        .set_kcs_enabled(true)
        .await?
        .is_none());

    let null = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(Value::Null), false),
    )
    .await?;
    assert!(null
        .oem_lenovo()?
        .ok_or("Lenovo OEM data missing")?
        .set_kcs_enabled(true)
        .await?
        .is_none());
    Ok(())
}

#[test]
async fn lenovo_security_fw_rollback_disabled() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!("Disabled")), true),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.security_id,
        security_payload(&ids, "Disabled"),
    ));

    let lenovo = manager.oem_lenovo()?.unwrap();
    let security = lenovo.security().await?.unwrap();
    assert_eq!(security.fw_rollback(), Some(FwRollbackState::Disabled));

    Ok(())
}

#[test]
async fn lenovo_security_updates_firmware_rollback() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!("Disabled")), true),
    )
    .await?;
    bmc.expect(Expect::get(
        &ids.security_id,
        security_payload(&ids, "Disabled"),
    ));
    let security = manager
        .oem_lenovo()?
        .ok_or("Lenovo OEM data missing")?
        .security()
        .await?
        .ok_or("Lenovo security service missing")?;
    let request = json!({ "Configurator": { "FWRollback": "Enabled" } });
    bmc.expect(Expect::update(
        &ids.security_id,
        &request,
        security_payload(&ids, "Enabled"),
    ));

    let ModificationResponse::Entity(updated) =
        security.set_fw_rollback(FwRollbackState::Enabled).await?
    else {
        return Err("expected updated Lenovo security service".into());
    };
    assert_eq!(updated.fw_rollback(), Some(FwRollbackState::Enabled));
    bmc.expect(Expect::update_empty(&ids.security_id, &request));
    assert_empty(updated.set_fw_rollback(FwRollbackState::Enabled).await?);
    Ok(())
}

#[test]
async fn manager_without_lenovo_oem_returns_not_available() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload_without_lenovo(&ids)).await?;

    assert!(manager.oem_lenovo()?.is_none());

    Ok(())
}

#[test]
async fn manager_with_null_lenovo_oem_returns_not_available() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload_without_lenovo(&ids),
            &json!({ "Oem": { "Lenovo": null } }),
        ]),
    )
    .await?;

    assert!(manager.oem_lenovo()?.is_none());

    Ok(())
}

#[test]
async fn lenovo_oem_without_security_returns_not_available() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!("Disabled")), false),
    )
    .await?;

    let lenovo = manager.oem_lenovo()?.unwrap();
    assert!(lenovo.security().await?.is_none());

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
    security_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    let manager_id = format!("{managers_id}/1");
    let security_id = format!("{manager_id}/Oem/Lenovo/Security");
    Ids {
        root_id,
        managers_id,
        manager_id,
        security_id,
    }
}

fn manager_payload(ids: &Ids, kcs_enabled: Option<Value>, include_security: bool) -> Value {
    let base = json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "Status": { "State": "Enabled" },
    });
    let mut lenovo = json!({
        ODATA_TYPE: "#LenovoManager.v1_0_0.LenovoManagerProperties"
    });
    if let Some(v) = kcs_enabled {
        lenovo["KCSEnabled"] = v;
    }
    if include_security {
        lenovo["Security"] = json!({ ODATA_ID: &ids.security_id });
    }
    let oem = json!({
        "Oem": {
            "Lenovo": lenovo
        }
    });
    json_merge([&base, &oem])
}

fn manager_payload_without_lenovo(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "Status": { "State": "Enabled" },
        "Oem": {}
    })
}

fn security_payload(ids: &Ids, fw_rollback: &str) -> Value {
    json!({
        ODATA_ID: &ids.security_id,
        ODATA_TYPE: SECURITY_SERVICE_DATA_TYPE,
        "Id": "Security",
        "Name": "Security",
        "Status": { "State": "Enabled" },
        "Configurator": {
            "FWRollback": fw_rollback
        }
    })
}
