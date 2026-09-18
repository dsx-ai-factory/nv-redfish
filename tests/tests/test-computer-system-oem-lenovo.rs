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
//! Integration tests for Lenovo ComputerSystem OEM support.

use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::oem::lenovo::boot_manager::BootOrderKind;
use nv_redfish::oem::lenovo::boot_manager::LenovoBootManagerUpdate;
use nv_redfish::oem::lenovo::computer_system::FpMode;
use nv_redfish::oem::lenovo::computer_system::PortSwitchingTo;
use nv_redfish::oem::lenovo::SystemResetType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::assert_empty;
use nv_redfish_tests::expect_redfish_reset_action;
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
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_19_0.ComputerSystem";
const LENOVO_BOOT_COLLECTION_DATA_TYPE: &str =
    "#LenovoBootManagerCollection.LenovoBootManagerCollection";
const LENOVO_BOOT_DATA_TYPE: &str = "#LenovoBootManager.v1_0_0.LenovoBootManager";

#[test]
async fn lenovo_computer_system_usb_management_fields() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                ODATA_TYPE: "#LenovoComputerSystem.v1_0_0.LenovoSystemProperties",
                "USBManagementPortAssignment": {
                    "FPMode": "Server",
                    "PortSwitchingTo": "Server"
                }
            })),
        ),
    )
    .await?;

    let lenovo = system.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.front_panel_mode(), Some(FpMode::Server));
    assert_eq!(lenovo.port_switching_to(), Some(PortSwitchingTo::Server));

    Ok(())
}

#[test]
async fn lenovo_computer_system_front_panel_usb_variant() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                Some(json!({
                    "FPMode": "BMC",
                    "PortSwitchingTo": "BMC"
                })),
                None,
            )),
        ),
    )
    .await?;

    let lenovo = system.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.front_panel_mode(), Some(FpMode::Bmc));
    assert_eq!(lenovo.port_switching_to(), Some(PortSwitchingTo::Bmc));

    Ok(())
}

#[test]
async fn lenovo_computer_system_prefers_usb_management_port_assignment(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                Some(json!({
                    "FPMode": "BMC",
                    "PortSwitchingTo": "BMC"
                })),
                Some(json!({
                    "FPMode": "Server",
                    "PortSwitchingTo": "Server"
                })),
            )),
        ),
    )
    .await?;

    // USBManagementPortAssignment is currently primary when both are present.
    let lenovo = system.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.front_panel_mode(), Some(FpMode::Server));
    assert_eq!(lenovo.port_switching_to(), Some(PortSwitchingTo::Server));

    Ok(())
}

#[test]
async fn lenovo_computer_system_null_usb_management_falls_back_to_front_panel(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                Some(json!({
                    "FPMode": "BMC",
                    "PortSwitchingTo": "BMC"
                })),
                Some(Value::Null),
            )),
        ),
    )
    .await?;
    let lenovo = system.oem_lenovo()?.ok_or("Lenovo OEM data missing")?;
    assert_eq!(lenovo.front_panel_mode(), Some(FpMode::Bmc));
    assert_eq!(lenovo.port_switching_to(), Some(PortSwitchingTo::Bmc));
    bmc.expect(Expect::update(
        &ids.system_id,
        json!({
            "Oem": {
                "Lenovo": {
                    "FrontPanelUSB": {
                        "FPMode": "Server",
                        "PortSwitchingTo": "Server"
                    }
                }
            }
        }),
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                Some(json!({
                    "FPMode": "Server",
                    "PortSwitchingTo": "Server"
                })),
                Some(Value::Null),
            )),
        ),
    ));

    assert!(matches!(
        lenovo
            .set_usb_management_port(FpMode::Server, PortSwitchingTo::Server)
            .await?,
        Some(ModificationResponse::Entity(_))
    ));
    Ok(())
}

#[test]
async fn lenovo_computer_system_both_variants_absent() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                ODATA_TYPE: "#LenovoComputerSystem.v1_0_0.LenovoSystemProperties"
            })),
        ),
    )
    .await?;

    let lenovo = system.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.front_panel_mode(), None);
    assert_eq!(lenovo.port_switching_to(), None);
    assert!(lenovo
        .set_usb_management_port(FpMode::Server, PortSwitchingTo::Server)
        .await?
        .is_none());
    Ok(())
}

#[test]
async fn lenovo_computer_system_partial_variant_fields() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                Some(json!({
                    "FPMode": "Shared"
                })),
                None,
            )),
        ),
    )
    .await?;

    let lenovo = system.oem_lenovo()?.unwrap();
    assert_eq!(lenovo.front_panel_mode(), Some(FpMode::Shared));
    assert_eq!(lenovo.port_switching_to(), None);
    Ok(())
}

#[test]
async fn lenovo_computer_system_updates_front_panel_usb_variant() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                Some(json!({
                    "FPMode": "Shared",
                    "PortSwitchingTo": "Server"
                })),
                None,
            )),
        ),
    )
    .await?;
    let updated_payload = system_payload(
        &ids,
        Some(lenovo_oem_payload(
            Some(json!({
                "FPMode": "Server",
                "PortSwitchingTo": "Server"
            })),
            None,
        )),
    );
    let request = json!({
        "Oem": {
            "Lenovo": {
                "FrontPanelUSB": {
                    "FPMode": "Server",
                    "PortSwitchingTo": "Server"
                }
            }
        }
    });
    bmc.expect(Expect::update(&ids.system_id, &request, updated_payload));

    let lenovo = system.oem_lenovo()?.ok_or("Lenovo OEM data missing")?;
    let ModificationResponse::Entity(updated) = lenovo
        .set_usb_management_port(FpMode::Server, PortSwitchingTo::Server)
        .await?
        .ok_or("Lenovo USB management unavailable")?
    else {
        return Err("expected updated computer system".into());
    };
    assert_eq!(
        updated.oem_lenovo()?.and_then(|oem| oem.front_panel_mode()),
        Some(FpMode::Server)
    );
    let updated_lenovo = updated
        .oem_lenovo()?
        .ok_or("updated Lenovo OEM data missing")?;
    bmc.expect(Expect::update_empty(&ids.system_id, &request));
    assert_empty(
        updated_lenovo
            .set_usb_management_port(FpMode::Server, PortSwitchingTo::Server)
            .await?
            .ok_or("Lenovo USB management unavailable")?,
    );
    Ok(())
}

#[test]
async fn lenovo_computer_system_updates_usb_management_variant() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(lenovo_oem_payload(
                None,
                Some(json!({
                    "FPMode": "Server",
                    "PortSwitchingTo": "Server"
                })),
            )),
        ),
    )
    .await?;
    let updated_payload = system_payload(
        &ids,
        Some(lenovo_oem_payload(
            None,
            Some(json!({
                "FPMode": "Shared",
                "PortSwitchingTo": "Server"
            })),
        )),
    );
    bmc.expect(Expect::update(
        &ids.system_id,
        json!({
            "Oem": {
                "Lenovo": {
                    "USBManagementPortAssignment": {
                        "FPMode": "Shared",
                        "PortSwitchingTo": "Server"
                    }
                }
            }
        }),
        updated_payload,
    ));

    let lenovo = system.oem_lenovo()?.ok_or("Lenovo OEM data missing")?;
    let ModificationResponse::Entity(updated) = lenovo
        .set_usb_management_port(FpMode::Shared, PortSwitchingTo::Server)
        .await?
        .ok_or("Lenovo USB management unavailable")?
    else {
        return Err("expected updated computer system".into());
    };
    assert_eq!(
        updated.oem_lenovo()?.and_then(|oem| oem.front_panel_mode()),
        Some(FpMode::Shared)
    );
    Ok(())
}

#[test]
async fn lenovo_system_reset_uses_advertised_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let action_target = format!(
        "{}/Actions/Oem/LenovoComputerSystem.SystemReset",
        ids.system_id
    );
    let system = get_system(
        bmc.clone(),
        &ids,
        json_merge([
            &system_payload(&ids, None),
            &json!({
                "Actions": {
                    "Oem": {
                        "#LenovoComputerSystem.SystemReset": {
                            "target": &action_target,
                            "title": "SystemReset"
                        }
                    }
                }
            }),
        ]),
    )
    .await?;
    expect_redfish_reset_action(&bmc, &action_target, Some("ACPowerCycle"));

    let actions = system
        .oem_lenovo_actions()?
        .ok_or("Lenovo OEM actions missing")?;
    assert!(matches!(
        actions.system_reset(SystemResetType::AcPowerCycle).await?,
        ModificationResponse::Entity(())
    ));
    Ok(())
}

#[test]
async fn lenovo_system_reset_requires_advertised_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc,
        &ids,
        json_merge([
            &system_payload(&ids, None),
            &json!({ "Actions": { "Oem": {} } }),
        ]),
    )
    .await?;
    let actions = system
        .oem_lenovo_actions()?
        .ok_or("Lenovo OEM actions missing")?;

    assert!(matches!(
        actions.system_reset(SystemResetType::AcPowerCycle).await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));
    Ok(())
}

#[test]
async fn lenovo_legacy_boot_settings_are_typed_and_updatable() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let settings_id = format!("{}/Oem/Lenovo/BootSettings", ids.system_id);
    let general_id = format!("{settings_id}/BootOrder.BootOrder");
    let network_id = format!("{settings_id}/BootOrder.NetworkBootOrder");
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                ODATA_TYPE: "#LenovoComputerSystem.v1_0_0.LenovoSystemProperties",
                "BootSettings": { ODATA_ID: &settings_id }
            })),
        ),
    )
    .await?;
    bmc.expect(Expect::get(
        &settings_id,
        json!({
            ODATA_ID: &settings_id,
            ODATA_TYPE: LENOVO_BOOT_COLLECTION_DATA_TYPE,
            "Name": "BootSettings",
            "Members": [
                { ODATA_ID: &network_id },
                { ODATA_ID: &general_id }
            ]
        }),
    ));
    let lenovo = system.oem_lenovo()?.ok_or("Lenovo OEM data missing")?;
    let settings = lenovo
        .boot_settings()
        .await?
        .ok_or("Lenovo boot settings missing")?;
    bmc.expect(Expect::get(
        &general_id,
        boot_manager_payload(
            &general_id,
            &["Hard Disk", "Network"],
            &["Hard Disk", "Network"],
            &["Network", "Hard Disk"],
        ),
    ));
    let general = settings
        .boot_order(BootOrderKind::General)
        .await?
        .ok_or("general boot order missing")?;
    bmc.expect(Expect::get(
        &network_id,
        boot_manager_payload(
            &network_id,
            &["HTTP IPv4 Nvidia Adapter"],
            &["HTTP IPv4 Nvidia Adapter"],
            &["HTTP IPv4 Nvidia Adapter"],
        ),
    ));
    let network = settings
        .boot_order(BootOrderKind::Network)
        .await?
        .ok_or("network boot order missing")?;
    assert_eq!(
        string_refs(general.current()),
        Some(vec!["Hard Disk", "Network"])
    );
    assert_eq!(
        string_refs(network.supported()),
        Some(vec!["HTTP IPv4 Nvidia Adapter"])
    );

    let update = LenovoBootManagerUpdate::builder()
        .with_boot_order_next(vec!["Network".into(), "Hard Disk".into()])
        .build();
    let request = json!({ "BootOrderNext": ["Network", "Hard Disk"] });
    bmc.expect(Expect::update(
        &general_id,
        &request,
        boot_manager_payload(
            &general_id,
            &["Hard Disk", "Network"],
            &["Network", "Hard Disk"],
            &["Network", "Hard Disk"],
        ),
    ));
    let ModificationResponse::Entity(updated) = general.update(&update).await? else {
        return Err("expected updated Lenovo boot order".into());
    };
    assert_eq!(
        string_refs(updated.next()),
        Some(vec!["Network", "Hard Disk"])
    );
    bmc.expect(Expect::update_empty(&general_id, &request));
    assert_empty(updated.update(&update).await?);
    Ok(())
}

#[test]
async fn system_without_lenovo_oem_returns_not_available() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc.clone(), &ids, system_payload(&ids, None)).await?;

    assert!(system.oem_lenovo()?.is_none());

    Ok(())
}

#[test]
async fn system_with_null_lenovo_oem_returns_not_available() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc.clone(), &ids, system_payload(&ids, Some(json!(null)))).await?;

    assert!(system.oem_lenovo()?.is_none());

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

    let systems = root.systems().await?.unwrap();
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single system must exist"))
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
            "Systems": { ODATA_ID: &ids.systems_id },
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
    systems_id: String,
    system_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/1");
    Ids {
        root_id,
        systems_id,
        system_id,
    }
}

fn system_payload(ids: &Ids, lenovo_oem: Option<Value>) -> Value {
    let base = json!({
        ODATA_ID: &ids.system_id,
        ODATA_TYPE: SYSTEM_DATA_TYPE,
        "Id": "1",
        "Name": "ComputerSystem",
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    });
    let oem = lenovo_oem.map_or_else(
        || json!({}),
        |lenovo| {
            json!({
                "Oem": {
                    "Lenovo": lenovo
                }
            })
        },
    );
    json_merge([&base, &oem])
}

fn lenovo_oem_payload(front_panel_usb: Option<Value>, usb_management: Option<Value>) -> Value {
    let mut payload = json!({
        ODATA_TYPE: "#LenovoComputerSystem.v1_0_0.LenovoSystemProperties",
    });
    if let Some(front_panel_usb) = front_panel_usb {
        payload["FrontPanelUSB"] = front_panel_usb;
    }
    if let Some(usb_management) = usb_management {
        payload["USBManagementPortAssignment"] = usb_management;
    }
    payload
}

fn boot_manager_payload(id: &str, current: &[&str], next: &[&str], supported: &[&str]) -> Value {
    json!({
        ODATA_ID: id,
        ODATA_TYPE: LENOVO_BOOT_DATA_TYPE,
        "Id": id.rsplit('/').next().unwrap_or(id),
        "Name": id.rsplit('/').next().unwrap_or(id),
        "BootOrderCurrent": current,
        "BootOrderNext": next,
        "BootOrderSupported": supported
    })
}

fn string_refs(values: Option<&[String]>) -> Option<Vec<&str>> {
    values.map(|values| values.iter().map(String::as_str).collect())
}
