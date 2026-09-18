// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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
//! Integration tests of BIOS support.

use nv_redfish::computer_system::AttributesUpdate;
use nv_redfish::computer_system::Bios;
use nv_redfish::computer_system::BiosUpdate;
use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::ServiceRoot;
use nv_redfish_core::EdmPrimitiveType;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const COMPUTER_SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_20_1.ComputerSystem";
const BIOS_DATA_TYPE: &str = "#Bios.v1_2_1.Bios";

#[test]
async fn bios_dynamic_attribute_update_serializes_and_redacts_debug() {
    const SECRET: &str = "bios-secret-sentinel";

    let values = HashMap::from([
        (
            "BootMode".to_string(),
            Some(EdmPrimitiveType::String("Uefi".to_string())),
        ),
        (
            "SetupPassword".to_string(),
            Some(EdmPrimitiveType::String(SECRET.to_string())),
        ),
    ]);
    let update = BiosUpdate::builder()
        .with_attributes(
            AttributesUpdate::builder()
                .with_dynamic_properties(values)
                .build(),
        )
        .build();

    assert_eq!(
        serde_json::to_value(&update).expect("BIOS update must serialize"),
        json!({
            "Attributes": {
                "BootMode": "Uefi",
                "SetupPassword": SECRET,
            }
        })
    );
    let debug = format!("{update:?}");
    assert!(!debug.contains(SECRET));
    assert!(debug.contains("dynamic_properties: \"<redacted>\""));
}

// Test 1: basic BIOS retrieval via bios() and EdmPrimitiveType mapping.
#[test]
async fn bios_basic_retrieval_and_types() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let system = get_computer_system(bmc.clone(), &ids, "Generic").await?;

    // Prepare BIOS payload with various attribute value types.
    bmc.expect(Expect::get(
        &ids.bios_id,
        json!({
            ODATA_ID: &ids.bios_id,
            ODATA_TYPE: BIOS_DATA_TYPE,
            "Id": "Bios",
            "Name": "BIOS Settings",
            "Attributes": {
                "BootMode": "Uefi",          // String
                "WatchdogTimeout": 5,        // Integer
                "Field Mode": false,         // Boolean
                "PowerCapping": 125.5,       // Decimal (floating point)
                "SetupPassword": null,       // password-like, write-only
            }
        }),
    ));

    let bios: Bios<Bmc> = system.bios().await?.unwrap();
    let raw = bios.raw();
    let attrs = raw
        .attributes
        .as_ref()
        .ok_or("attributes must be present")?;
    let map = &attrs.dynamic_properties;

    // Ensure all non-null attributes are present.
    assert!(map.contains_key("BootMode"));
    assert!(map.contains_key("WatchdogTimeout"));
    assert!(map.contains_key("PowerCapping"));

    // Verify underlying primitive types.
    let boot_mode = map.get("BootMode").expect("BootMode must exist");
    assert!(matches!(
        boot_mode,
        Some(EdmPrimitiveType::String(s)) if s == "Uefi"
    ));

    let watchdog_timeout = map
        .get("WatchdogTimeout")
        .expect("WatchdogTimeout must exist");
    assert!(matches!(watchdog_timeout, Some(EdmPrimitiveType::Integer(i)) if *i == 5));

    let power_capping = map.get("PowerCapping").expect("PowerCapping must exist");
    assert!(
        matches!(power_capping, Some(EdmPrimitiveType::Decimal(v)) if (v - 125.5).abs() < f64::EPSILON)
    );

    // Password-like attribute that is not present should be handled gracefully.
    assert!(bios.attribute("SetupPassword").is_some_and(|v| v.is_null()));

    Ok(())
}

// Test 2: BiosAttributeRef::string_value behavior for string vs non-string.
#[test]
async fn bios_attribute_string_value() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let system = get_computer_system(bmc.clone(), &ids, "Generic").await?;
    bmc.expect(Expect::get(
        &ids.bios_id,
        json!({
            ODATA_ID: &ids.bios_id,
            ODATA_TYPE: BIOS_DATA_TYPE,
            "Id": "Bios",
            "Name": "BIOS Settings",
            "Attributes": {
                "BootMode": "Uefi",
                "WatchdogTimeout": 5
            }
        }),
    ));

    let bios: Bios<Bmc> = system.bios().await?.unwrap();

    let boot_mode = bios
        .attribute("BootMode")
        .ok_or("BootMode attribute must exist")?;
    assert_eq!(boot_mode.str_value(), Some("Uefi"));

    let watchdog = bios
        .attribute("WatchdogTimeout")
        .ok_or("WatchdogTimeout attribute must exist")?;
    assert!(watchdog.str_value().is_none());

    Ok(())
}

// Test 3: missing or empty Attributes handling.
#[test]
async fn bios_missing_or_empty_attributes() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let system = get_computer_system(bmc.clone(), &ids, "Generic").await?;

    // Case A: no Attributes property at all.
    bmc.expect(Expect::get(
        &ids.bios_id,
        json!({
            ODATA_ID: &ids.bios_id,
            ODATA_TYPE: BIOS_DATA_TYPE,
            "Id": "Bios",
            "Name": "BIOS Settings"
        }),
    ));
    let bios_no_attrs: Bios<Bmc> = system.bios().await?.unwrap();
    assert!(bios_no_attrs.raw().attributes.is_none());
    assert!(bios_no_attrs.attribute("Anything").is_none());

    // Case B: empty Attributes object.
    bmc.expect(Expect::get(
        &ids.bios_id,
        json!({
            ODATA_ID: &ids.bios_id,
            ODATA_TYPE: BIOS_DATA_TYPE,
            "Id": "Bios",
            "Name": "BIOS Settings",
            "Attributes": {}
        }),
    ));
    let bios_empty_attrs: Bios<Bmc> = system.bios().await?.unwrap();
    let raw = bios_empty_attrs.raw();
    let attrs = raw.attributes.as_ref().unwrap();
    assert!(attrs.dynamic_properties.is_empty());
    assert!(bios_empty_attrs.attribute("Anything").is_none());

    Ok(())
}

#[test]
async fn bios_update_uses_live_resource_and_preserves_entity() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let system = get_computer_system(bmc.clone(), &ids, "Generic").await?;
    bmc.expect(Expect::get(
        &ids.bios_id,
        bios_payload(
            &ids.bios_id,
            json!({ "Attributes": { "BootMode": "Legacy" } }),
        ),
    ));
    let bios = system.bios().await?.ok_or("BIOS missing")?;
    let update = bios_update("BootMode", "Uefi");

    bmc.expect(Expect::update(
        &ids.bios_id,
        json!({ "Attributes": { "BootMode": "Uefi" } }),
        bios_payload(
            &ids.bios_id,
            json!({ "Attributes": { "BootMode": "Uefi" } }),
        ),
    ));

    let ModificationResponse::Entity(updated) = bios.update(&update).await? else {
        return Err("expected BIOS entity response".into());
    };
    let boot_mode = updated
        .attribute("BootMode")
        .ok_or("updated BootMode missing")?;
    assert_eq!(boot_mode.str_value(), Some("Uefi"));
    Ok(())
}

#[test]
async fn bios_settings_uses_advertised_reference_and_handles_absence(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let settings_id = format!("{}/Settings", ids.bios_id);
    let system = get_computer_system(bmc.clone(), &ids, "Generic").await?;
    bmc.expect(Expect::get(
        &ids.bios_id,
        bios_payload(
            &ids.bios_id,
            json!({
                "@Redfish.Settings": {
                    "SettingsObject": { ODATA_ID: &settings_id }
                }
            }),
        ),
    ));
    let bios = system.bios().await?.ok_or("BIOS missing")?;

    let settings_etag = "W/\"bios-settings\"";
    bmc.expect(Expect::get(
        &settings_id,
        bios_payload(
            &settings_id,
            json!({ "@odata.etag": settings_etag, "Attributes": {} }),
        ),
    ));
    let settings = bios.settings().await?.ok_or("BIOS settings missing")?;
    let update = bios_update("BootMode", "Uefi");
    bmc.expect(Expect::update(
        &settings_id,
        json!({ "Attributes": { "BootMode": "Uefi" } }),
        json!({ ODATA_ID: &settings_id }),
    ));
    bmc.expect(Expect::get(
        &settings_id,
        bios_payload(
            &settings_id,
            json!({ "Attributes": { "BootMode": "Uefi" } }),
        ),
    ));

    assert!(matches!(
        settings.update(&update).await?,
        ModificationResponse::Entity(_)
    ));

    let system = get_computer_system(bmc.clone(), &ids, "Generic").await?;
    bmc.expect(Expect::get(
        &ids.bios_id,
        bios_payload(&ids.bios_id, json!({})),
    ));
    let bios = system.bios().await?.ok_or("BIOS missing")?;
    assert!(bios.settings().await?.is_none());
    Ok(())
}

#[test]
async fn bios_actions_use_advertised_targets() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let reset_target = format!("{}/Actions/Bios.ResetBios", ids.bios_id);
    let password_target = format!("{}/Actions/Bios.ChangePassword", ids.bios_id);
    let system = get_computer_system(bmc.clone(), &ids, "Lenovo").await?;
    bmc.expect(Expect::get(
        &ids.bios_id,
        bios_payload(
            &ids.bios_id,
            json!({
                "Actions": {
                    "#Bios.ResetBios": {
                        "target": &reset_target,
                        "title": "ResetBios"
                    },
                    "#Bios.ChangePassword": {
                        "target": &password_target,
                        "title": "ChangePassword"
                    }
                }
            }),
        ),
    ));
    let bios = system.bios().await?.ok_or("BIOS missing")?;

    bmc.expect(Expect::action(&reset_target, json!({}), json!(null)));
    assert!(matches!(
        bios.reset().await?,
        ModificationResponse::Entity(())
    ));

    bmc.expect(Expect::action(
        &password_target,
        json!({
            "PasswordName": "UefiAdminPassword",
            "OldPassword": "old-secret",
            "NewPassword": "new-secret"
        }),
        json!(null),
    ));
    assert!(matches!(
        bios.change_password(
            "UefiAdminPassword".into(),
            Some("old-secret".into()),
            "new-secret".into(),
        )
        .await?,
        ModificationResponse::Entity(())
    ));
    Ok(())
}

#[test]
async fn bios_actions_must_be_advertised() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bios_ids();
    let system = get_computer_system(bmc.clone(), &ids, "Lenovo").await?;
    bmc.expect(Expect::get(
        &ids.bios_id,
        bios_payload(&ids.bios_id, json!({ "Actions": {} })),
    ));
    let bios = system.bios().await?.ok_or("BIOS missing")?;

    assert!(matches!(
        bios.reset().await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));
    assert!(matches!(
        bios.change_password(
            "UefiAdminPassword".into(),
            Some("old-secret".into()),
            "new-secret".into(),
        )
        .await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));
    Ok(())
}

fn bios_update(name: &str, value: &str) -> BiosUpdate {
    BiosUpdate::builder()
        .with_attributes(
            AttributesUpdate::builder()
                .with_dynamic_properties(HashMap::from([(
                    name.to_string(),
                    Some(EdmPrimitiveType::String(value.to_string())),
                )]))
                .build(),
        )
        .build()
}

fn bios_payload(id: &str, fields: serde_json::Value) -> serde_json::Value {
    nv_redfish_tests::json_merge([
        &json!({
            ODATA_ID: id,
            ODATA_TYPE: BIOS_DATA_TYPE,
            "Id": "Bios",
            "Name": "BIOS Settings",
        }),
        &fields,
    ])
}

struct BiosIds {
    root_id: ODataId,
    systems_id: String,
    system_id: String,
    bios_id: String,
}

fn bios_ids() -> BiosIds {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/System-1");
    let bios_id = format!("{system_id}/Bios");
    BiosIds {
        root_id,
        systems_id,
        system_id,
        bios_id,
    }
}

/// Helper that prepares a minimal service root and a single computer system
/// with an associated BIOS navigation property.
async fn get_computer_system(
    bmc: Arc<Bmc>,
    ids: &BiosIds,
    vendor: &str,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    // Service root with Systems nav property.
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
            "Vendor": vendor,
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));

    let service_root = ServiceRoot::new(bmc.clone()).await?;

    // Computer system collection with a single member, fetched via expand.
    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
            "Name": "Systems Collection",
            "Members": [
                {
                    ODATA_ID: &ids.system_id,
                }
            ],
        }),
    ));

    let systems = service_root.systems().await?.unwrap();

    // Individual computer system with Bios nav property.
    bmc.expect(Expect::get(
        &ids.system_id,
        json!({
            ODATA_ID: &ids.system_id,
            ODATA_TYPE: COMPUTER_SYSTEM_DATA_TYPE,
            "Id": "System-1",
            "Name": "System-1",
            "Bios": {
                ODATA_ID: &ids.bios_id,
            },
        }),
    ));

    let mut members = systems.members().await?;
    // We expect exactly one system in this helper.
    let system = members.pop().ok_or("no computer system members returned")?;
    Ok(system)
}
