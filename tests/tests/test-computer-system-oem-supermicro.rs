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

//! Integration tests for Supermicro ComputerSystem OEM support.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::oem::supermicro::fixed_boot_order::BootMode;
use nv_redfish::oem::supermicro::fixed_boot_order::SmcFixedBootOrderUpdate;
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
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_23_0.ComputerSystem";
const FIXED_BOOT_DATA_TYPE: &str = "#SmcFixedBootOrder.v1_0_0.SmcFixedBootOrder";

#[test]
async fn fixed_boot_order_is_typed_and_updatable() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc.clone(), &ids, system_payload(&ids, true, false)).await?;
    let smc = system
        .oem_supermicro()?
        .ok_or("expected Supermicro system OEM data")?;

    bmc.expect(Expect::get(
        &ids.fixed_boot_order_id,
        fixed_boot_order_payload(&ids, &["UEFI Hard Disk", "UEFI Network"], &["NIC 1"], false),
    ));
    let fixed = smc
        .fixed_boot_order()
        .await?
        .ok_or("expected fixed boot order")?;
    assert_eq!(fixed.boot_mode_selected(), Some(BootMode::Uefi));
    assert_eq!(
        fixed.fixed_boot_order(),
        Some(["UEFI Hard Disk".to_string(), "UEFI Network".to_string()].as_slice())
    );
    assert_eq!(
        fixed.fixed_boot_order_disabled_items(),
        Some(["Disabled".to_string()].as_slice())
    );
    assert_eq!(fixed.uefi_hard_disk(), None);

    bmc.expect(Expect::update(
        &ids.fixed_boot_order_id,
        json!({
            "FixedBootOrder": ["UEFI Network", "UEFI Hard Disk"],
            "UEFIHardDisk": ["ubuntu"],
            "UEFINetwork": ["NIC 1"]
        }),
        fixed_boot_order_payload(&ids, &["UEFI Network", "UEFI Hard Disk"], &["NIC 1"], true),
    ));
    let update = SmcFixedBootOrderUpdate::builder()
        .with_fixed_boot_order(vec![
            "UEFI Network".to_string(),
            "UEFI Hard Disk".to_string(),
        ])
        .with_uefi_hard_disk(vec!["ubuntu".to_string()])
        .with_uefi_network(vec!["NIC 1".to_string()])
        .build();
    let ModificationResponse::Entity(updated) = fixed.update(&update).await? else {
        return Err("expected updated fixed boot order".into());
    };
    assert_eq!(
        updated.fixed_boot_order(),
        Some(["UEFI Network".to_string(), "UEFI Hard Disk".to_string()].as_slice())
    );

    Ok(())
}

#[test]
async fn ac_power_cycle_uses_advertised_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc.clone(), &ids, system_payload(&ids, true, true)).await?;
    let actions = system
        .oem_supermicro_actions()?
        .ok_or("expected Supermicro AC-cycle action")?;

    bmc.expect(Expect::action(
        &ids.ac_cycle_target,
        json!({ "ResetType": "ACCycle" }),
        json!(null),
    ));
    assert!(matches!(
        actions.ac_power_cycle().await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn ac_power_cycle_requires_advertisement() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc, &ids, system_payload(&ids, true, false)).await?;

    assert!(system.oem_supermicro_actions()?.is_none());

    Ok(())
}

#[test]
async fn system_without_supermicro_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc, &ids, system_payload(&ids, false, false)).await?;

    assert!(system.oem_supermicro()?.is_none());

    Ok(())
}

#[test]
async fn ipmi_host_interface_is_typed_and_updatable() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let initial = json_merge([
        &system_payload(&ids, true, false),
        &json!({
            ODATA_TYPE: "#ComputerSystem.v1_25_0.ComputerSystem",
            "@odata.etag": "\"system-etag\"",
            "IPMIHostInterface": { "ServiceEnabled": true }
        }),
    ]);
    let system = get_system(bmc.clone(), &ids, initial).await?;
    assert_eq!(system.ipmi_host_interface_enabled(), Some(true));

    let updated = json_merge([
        &system_payload(&ids, true, false),
        &json!({
            ODATA_TYPE: "#ComputerSystem.v1_25_0.ComputerSystem",
            "@odata.etag": "\"updated-system-etag\"",
            "IPMIHostInterface": { "ServiceEnabled": false }
        }),
    ]);
    bmc.expect(Expect::update(
        &ids.system_id,
        json!({ "IPMIHostInterface": { "ServiceEnabled": false } }),
        updated,
    ));
    let ModificationResponse::Entity(system) =
        system.set_ipmi_host_interface_enabled(false).await?
    else {
        return Err("expected updated ComputerSystem".into());
    };
    assert_eq!(system.ipmi_host_interface_enabled(), Some(false));

    Ok(())
}

async fn get_system(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: SYSTEM_COLLECTION_DATA_TYPE,
            "Id": "Systems",
            "Name": "Computer System Collection",
            "Members": [member]
        }),
    ));

    let systems = root.systems().await?.ok_or("expected Systems")?;
    let mut members = systems.members().await?;
    if members.len() != 1 {
        return Err("expected one ComputerSystem".into());
    }
    Ok(members.remove(0))
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
                "ExpandQuery": { "NoLinks": true }
            },
            "Systems": { ODATA_ID: &ids.systems_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id)
                }
            }
        }),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct Ids {
    root_id: ODataId,
    systems_id: String,
    system_id: String,
    fixed_boot_order_id: String,
    ac_cycle_target: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/1");
    let fixed_boot_order_id = format!("{system_id}/Oem/Supermicro/FixedBootOrder");
    let ac_cycle_target = format!("{system_id}/Actions/Oem/OemSystemExtensions.Reset");
    Ids {
        root_id,
        systems_id,
        system_id,
        fixed_boot_order_id,
        ac_cycle_target,
    }
}

fn system_payload(ids: &Ids, include_oem: bool, include_ac_cycle: bool) -> Value {
    let base = json!({
        ODATA_ID: &ids.system_id,
        ODATA_TYPE: SYSTEM_DATA_TYPE,
        "Id": "1",
        "Name": "System",
        "Status": { "Health": "OK", "State": "Enabled" },
        "Actions": {
            "Oem": if include_ac_cycle {
                json!({
                    "#OemSystemExtensions.Reset": {
                        "target": &ids.ac_cycle_target
                    }
                })
            } else {
                json!({})
            }
        }
    });
    if include_oem {
        json_merge([
            &base,
            &json!({
                "Oem": {
                    "Supermicro": {
                        "FixedBootOrder": {
                            ODATA_ID: &ids.fixed_boot_order_id
                        }
                    }
                }
            }),
        ])
    } else {
        base
    }
}

fn fixed_boot_order_payload(
    ids: &Ids,
    order: &[&str],
    network: &[&str],
    include_hard_disk: bool,
) -> Value {
    let mut payload = json!({
        ODATA_ID: &ids.fixed_boot_order_id,
        ODATA_TYPE: FIXED_BOOT_DATA_TYPE,
        "@odata.etag": "\"fixed-order-etag\"",
        "Id": "1",
        "Name": "Fixed Boot Order",
        "BootModeSelected": "UEFI",
        "FixedBootOrder": order,
        "FixedBootOrderDisabledItem": ["Disabled"],
        "UEFIAP": ["UEFI: Built-in EFI Shell"],
        "UEFIAPDisabledItem": ["Disabled"],
        "UEFINetwork": network,
        "UEFINetworkDisabledItem": ["Disabled"]
    });
    if include_hard_disk {
        payload["UEFIHardDisk"] = json!(["ubuntu"]);
        payload["UEFIHardDiskDisabledItem"] = json!(["Disabled"]);
    }
    payload
}
