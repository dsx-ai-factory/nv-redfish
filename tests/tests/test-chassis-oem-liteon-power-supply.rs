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
//! Integration tests for LiteOn OEM power supply links via chassis.

use nv_redfish::chassis::Chassis;
use nv_redfish::ServiceRoot;
use nv_redfish_core::LocationIndicatorActive;
use nv_redfish_core::ODataId;
use nv_redfish_tests::anonymous_1_9_service_root;
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

const CHASSIS_COLLECTION_DATA_TYPE: &str = "#ChassisCollection.ChassisCollection";
const CHASSIS_DATA_TYPE: &str = "#Chassis.v1_23_0.Chassis";
const POWER_SUBSYSTEM_DATA_TYPE: &str = "#PowerSubsystem.v1_1_0.PowerSubsystem";
const PSU_COLLECTION_DATA_TYPE: &str = "#PowerSupplyCollection.PowerSupplyCollection";
const LITEON_PSU_DATA_TYPE: &str = "#LiteonPowerSupply.v1_0_0.LiteonPowerSupply";

#[test]
async fn liteon_power_supply_fetch_location_indicator_led_object() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis =
        get_liteon_chassis(bmc.clone(), &ids, liteon_chassis_member(&ids, json!({}))).await?;
    expect_power_subsystem(bmc.clone(), &ids);
    expect_psu_collection(
        bmc.clone(),
        &ids,
        vec![format!("{}/0", ids.psu_collection_id)],
    );

    let links = chassis.oem_liteon_power_supply_links().await?.unwrap();
    let psu_id = format!("{}/0", ids.psu_collection_id);
    bmc.expect(Expect::get(
        &psu_id,
        json_merge([
            &psu_payload(&psu_id, "0", true),
            &json!({
                "LocationIndicatorActive": {
                    "FaultLed": "OFF",
                    "PowerLed": "Solid"
                }
            }),
        ]),
    ));
    let psu = links[0].fetch().await?;
    assert_eq!(
        psu.location_indicator_active,
        Some(Some(LocationIndicatorActive::LedIndicators {
            fault_led: "OFF".into(),
            power_led: "Solid".into(),
        }))
    );

    Ok(())
}

#[test]
async fn liteon_power_supply_links_happy_path() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis =
        get_liteon_chassis(bmc.clone(), &ids, liteon_chassis_member(&ids, json!({}))).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    expect_psu_collection(
        bmc.clone(),
        &ids,
        vec![format!("{}/0", ids.psu_collection_id)],
    );

    let links = chassis.oem_liteon_power_supply_links().await?.unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].odata_id().to_string(),
        format!("{}/0", ids.psu_collection_id)
    );

    // Verify fetch works on the link
    let psu_id = format!("{}/0", ids.psu_collection_id);
    bmc.expect(Expect::get(&psu_id, psu_payload(&psu_id, "0", true)));
    let psu = links[0].fetch().await?;
    assert_eq!(psu.power_state, Some(true));
    assert_eq!(
        psu.capacity_watts.as_ref().map(|v| v.as_deref()),
        Some(Some("5500"))
    );

    Ok(())
}

#[test]
async fn liteon_power_supply_links_multiple_psus() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis =
        get_liteon_chassis(bmc.clone(), &ids, liteon_chassis_member(&ids, json!({}))).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    expect_psu_collection(
        bmc.clone(),
        &ids,
        vec![
            format!("{}/0", ids.psu_collection_id),
            format!("{}/1", ids.psu_collection_id),
        ],
    );

    let links = chassis.oem_liteon_power_supply_links().await?.unwrap();
    assert_eq!(links.len(), 2);
    assert_eq!(
        links[0].odata_id().to_string(),
        format!("{}/0", ids.psu_collection_id)
    );
    assert_eq!(
        links[1].odata_id().to_string(),
        format!("{}/1", ids.psu_collection_id)
    );

    Ok(())
}

#[test]
async fn liteon_power_supply_links_wrong_manufacturer_returns_none() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_liteon_chassis(
        bmc.clone(),
        &ids,
        chassis_member(
            &ids,
            json!({
                "Manufacturer": "ACME Corp.",
                "PowerSubsystem": { ODATA_ID: &ids.power_subsystem_id }
            }),
        ),
    )
    .await?;

    let result = chassis.oem_liteon_power_supply_links().await?;
    assert!(result.is_none());

    Ok(())
}

#[test]
async fn liteon_power_supply_links_missing_manufacturer_returns_none(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_liteon_chassis(bmc.clone(), &ids, chassis_member(&ids, json!({}))).await?;

    let result = chassis.oem_liteon_power_supply_links().await?;
    assert!(result.is_none());

    Ok(())
}

#[test]
async fn liteon_power_supply_links_missing_power_subsystem_returns_none(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_liteon_chassis(
        bmc.clone(),
        &ids,
        json_merge([&json!({
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "powershelf",
            "Name": "powershelf",
            "ChassisType": "Shelf",
            "Manufacturer": "LITE-ON TECHNOLOGY CORP."
        })]),
    )
    .await?;

    let result = chassis.oem_liteon_power_supply_links().await?;
    assert!(result.is_none());

    Ok(())
}

#[test]
async fn liteon_power_supply_links_empty_collection() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis =
        get_liteon_chassis(bmc.clone(), &ids, liteon_chassis_member(&ids, json!({}))).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    expect_psu_collection(bmc.clone(), &ids, vec![]);

    let links = chassis.oem_liteon_power_supply_links().await?.unwrap();
    assert!(links.is_empty());

    Ok(())
}

#[test]
async fn liteon_location_indicator_active_is_ignored_on_oem_fetch() -> Result<(), Box<dyn StdError>>
{
    for expanded in [false, true] {
        for (indicator, _) in indicator_cases() {
            let bmc = Arc::new(Bmc::default());
            let ids = ids();
            let chassis =
                get_liteon_chassis(bmc.clone(), &ids, liteon_chassis_member(&ids, json!({})))
                    .await?;
            let psu_id = format!("{}/0", ids.psu_collection_id);
            let mut payload = psu_payload(&psu_id, "0", true);
            payload["SerialNumber"] = json!("PSU-123");
            if let Some(indicator) = indicator {
                payload["LocationIndicatorActive"] = indicator;
            }

            expect_power_subsystem(bmc.clone(), &ids);
            expect_psu_members(
                &bmc,
                &ids,
                vec![if expanded {
                    payload.clone()
                } else {
                    json!({ ODATA_ID: &psu_id })
                }],
            );
            let links = chassis.oem_liteon_power_supply_links().await?.unwrap();
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].odata_id().to_string(), psu_id);
            bmc.expect(Expect::get(&psu_id, payload));
            let psu = links[0].fetch().await?;
            assert_eq!(psu.id, "0");
            assert_eq!(psu.name, "Power Supply 0");
            assert_eq!(
                psu.capacity_watts.as_ref().and_then(Option::as_deref),
                Some("5500")
            );
            assert_eq!(psu.power_state, Some(true));
            assert_eq!(
                psu.serial_number.as_ref().and_then(Option::as_deref),
                Some("PSU-123")
            );
        }
    }
    Ok(())
}

#[test]
async fn liteon_location_indicator_active_is_patched_on_standard_fetch(
) -> Result<(), Box<dyn StdError>> {
    for expanded in [false, true] {
        for root_vendor in [false, true] {
            for (indicator, expected) in indicator_cases() {
                let bmc = Arc::new(Bmc::default());
                let ids = ids();
                // Exercise both service-root detection and the manufacturer fallback.
                let (member, root_fields) = if root_vendor {
                    (
                        chassis_member(
                            &ids,
                            json!({
                                "PowerSubsystem": { ODATA_ID: &ids.power_subsystem_id }
                            }),
                        ),
                        json!({ "Vendor": "LITE-ON TECHNOLOGY CORP." }),
                    )
                } else {
                    (liteon_chassis_member(&ids, json!({})), json!({}))
                };
                let chassis = get_chassis(bmc.clone(), &ids, member, root_fields).await?;
                let psu_id = format!("{}/0", ids.psu_collection_id);
                let mut payload = json!({
                    ODATA_ID: &psu_id,
                    ODATA_TYPE: PSU_DATA_TYPE,
                    "Id": "0",
                    "Name": "Power Supply 0",
                    "SerialNumber": "PSU-123"
                });
                if let Some(indicator) = indicator {
                    payload["LocationIndicatorActive"] = indicator;
                }
                expect_power_subsystem(bmc.clone(), &ids);
                expect_psu_members(
                    &bmc,
                    &ids,
                    vec![if expanded {
                        payload.clone()
                    } else {
                        json!({ ODATA_ID: &psu_id })
                    }],
                );
                if !expanded {
                    bmc.expect(Expect::get(&psu_id, payload));
                }
                let supplies = chassis.power_supplies().await?;
                assert_eq!(supplies.len(), 1);
                let psu = supplies[0].raw();
                assert_eq!(psu.location_indicator_active, expected);
                assert_eq!(
                    psu.serial_number.as_ref().and_then(Option::as_deref),
                    Some("PSU-123")
                );
            }
        }
    }
    Ok(())
}

#[test]
async fn location_indicator_active_object_is_rejected_on_other_platforms(
) -> Result<(), Box<dyn StdError>> {
    for expanded in [false, true] {
        let bmc = Arc::new(Bmc::default());
        let ids = ids();
        let chassis = get_liteon_chassis(
            bmc.clone(),
            &ids,
            chassis_member(
                &ids,
                json!({
                    "Manufacturer": "ACME Corp.",
                    "PowerSubsystem": { ODATA_ID: &ids.power_subsystem_id }
                }),
            ),
        )
        .await?;
        let psu_id = format!("{}/0", ids.psu_collection_id);
        let payload = json!({
            ODATA_ID: &psu_id,
            ODATA_TYPE: PSU_DATA_TYPE,
            "Id": "0",
            "Name": "Power Supply 0",
            "LocationIndicatorActive": { "FaultLed": "OFF", "PowerLed": "Solid" }
        });
        expect_power_subsystem(bmc.clone(), &ids);
        expect_psu_members(
            &bmc,
            &ids,
            vec![if expanded {
                payload.clone()
            } else {
                json!({ ODATA_ID: &psu_id })
            }],
        );
        if !expanded {
            bmc.expect(Expect::get(&psu_id, payload));
        }
        let err = match chassis.power_supplies().await {
            Ok(_) => panic!("invalid indicator must be rejected on other platforms"),
            Err(err) => err.to_string(),
        };
        assert!(
            err.contains("expected a boolean"),
            "unexpected error: {}",
            err
        );
    }
    Ok(())
}

fn indicator_cases() -> Vec<(Option<Value>, Option<Option<bool>>)> {
    vec![
        (
            Some(json!({ "FaultLed": "OFF", "PowerLed": "Solid" })),
            None,
        ),
        (Some(json!(true)), Some(Some(true))),
        (Some(json!(false)), Some(Some(false))),
        (Some(Value::Null), Some(None)),
        (None, None),
    ]
}

// --- Helpers ---

struct Ids {
    root_id: ODataId,
    chassis_collection_id: String,
    chassis_id: String,
    power_subsystem_id: String,
    psu_collection_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let chassis_collection_id = format!("{root_id}/Chassis");
    let chassis_id = format!("{chassis_collection_id}/powershelf");
    let power_subsystem_id = format!("{chassis_id}/PowerSubsystem");
    let psu_collection_id = format!("{power_subsystem_id}/PowerSupplies");
    Ids {
        root_id,
        chassis_collection_id,
        chassis_id,
        power_subsystem_id,
        psu_collection_id,
    }
}

fn chassis_member(ids: &Ids, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: &ids.chassis_id,
        ODATA_TYPE: CHASSIS_DATA_TYPE,
        "Id": "powershelf",
        "Name": "powershelf",
        "ChassisType": "Shelf"
    });
    json_merge([&base, &fields])
}

fn liteon_chassis_member(ids: &Ids, extra: Value) -> Value {
    chassis_member(
        ids,
        json_merge([
            &json!({
                "Manufacturer": "LITE-ON TECHNOLOGY CORP.",
                "PowerSubsystem": { ODATA_ID: &ids.power_subsystem_id }
            }),
            &extra,
        ]),
    )
}

fn psu_payload(psu_id: &str, id: &str, power_state: bool) -> Value {
    json!({
        ODATA_ID: psu_id,
        ODATA_TYPE: LITEON_PSU_DATA_TYPE,
        "Id": id,
        "Name": format!("Power Supply {id}"),
        "Manufacturer": "LITE-ON TECHNOLOGY CORP.",
        "Model": "SP-2552-1R",
        "PowerState": power_state,
        "CapacityWatts": "5500",
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    })
}

async fn get_liteon_chassis(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
) -> Result<Chassis<Bmc>, Box<dyn StdError>> {
    get_chassis(bmc, ids, member, json!({})).await
}

async fn get_chassis(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
    root_fields: Value,
) -> Result<Chassis<Bmc>, Box<dyn StdError>> {
    let service_root = expect_service_root(bmc.clone(), ids, root_fields).await?;
    bmc.expect(Expect::get(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [member]
        }),
    ));
    let collection = service_root.chassis().await?.unwrap();
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single chassis must exist"))
}

async fn expect_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        anonymous_1_9_service_root(
            &ids.root_id,
            json_merge([
                &json!({
                    "Chassis": { ODATA_ID: &ids.chassis_collection_id }
                }),
                &fields,
            ]),
        ),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

fn expect_power_subsystem(bmc: Arc<Bmc>, ids: &Ids) {
    bmc.expect(Expect::get(
        &ids.power_subsystem_id,
        json!({
            ODATA_ID: &ids.power_subsystem_id,
            ODATA_TYPE: POWER_SUBSYSTEM_DATA_TYPE,
            "Id": "PowerSubsystem",
            "Name": "Power Subsystem",
            "PowerSupplies": { ODATA_ID: &ids.psu_collection_id }
        }),
    ));
}

fn expect_psu_collection(bmc: Arc<Bmc>, ids: &Ids, psu_ids: Vec<String>) {
    let members: Vec<Value> = psu_ids.iter().map(|id| json!({ ODATA_ID: id })).collect();
    expect_psu_members(&bmc, ids, members);
}

fn expect_psu_members(bmc: &Bmc, ids: &Ids, members: Vec<Value>) {
    bmc.expect(Expect::get(
        &ids.psu_collection_id,
        json!({
            ODATA_ID: &ids.psu_collection_id,
            ODATA_TYPE: PSU_COLLECTION_DATA_TYPE,
            "Id": "PowerSupplies",
            "Name": "Power Supply Collection",
            "Members": members
        }),
    ));
}
