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
//! Integration tests for the Delta Energy Systems power supply OEM extension.

use nv_redfish::chassis::Chassis;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::anonymous_1_9_service_root;
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
const PSU_DATA_TYPE: &str = "#PowerSupply.v1_5_0.PowerSupply";
const DELTA_PSU_OEM_DATA_TYPE: &str = "#DeltaEnergySystemsPowerSupply.v1_0_0.PowerSupply";

#[test]
async fn delta_power_supply_oem_reports_power_state() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_delta_chassis(bmc.clone(), &ids).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    let psu_id = format!("{}/3", ids.psu_collection_id);
    expect_psu_collection(bmc.clone(), &ids, vec![psu_id.clone()]);
    bmc.expect(Expect::get(
        &psu_id,
        delta_psu_payload(&psu_id, "PowerSupplyUnit 3", Some(true), Some(0)),
    ));

    let supplies = chassis.power_supplies().await?;
    assert_eq!(supplies.len(), 1);

    let oem = supplies[0]
        .oem_delta()?
        .expect("Delta OEM extension must be present");
    assert_eq!(oem.power(), Some(true));
    assert_eq!(oem.fan_speed_target(), Some(0));

    Ok(())
}

#[test]
async fn delta_power_supply_oem_reports_power_off() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_delta_chassis(bmc.clone(), &ids).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    let psu_id = format!("{}/4", ids.psu_collection_id);
    expect_psu_collection(bmc.clone(), &ids, vec![psu_id.clone()]);
    bmc.expect(Expect::get(
        &psu_id,
        delta_psu_payload(&psu_id, "PowerSupplyUnit 4", Some(false), Some(30)),
    ));

    let supplies = chassis.power_supplies().await?;
    let oem = supplies[0]
        .oem_delta()?
        .expect("Delta OEM extension must be present");
    assert_eq!(oem.power(), Some(false));
    assert_eq!(oem.fan_speed_target(), Some(30));

    Ok(())
}

#[test]
async fn delta_power_supply_without_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_delta_chassis(bmc.clone(), &ids).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    let psu_id = format!("{}/3", ids.psu_collection_id);
    expect_psu_collection(bmc.clone(), &ids, vec![psu_id.clone()]);
    // A power supply that carries no Delta OEM object.
    bmc.expect(Expect::get(
        &psu_id,
        json!({
            ODATA_ID: &psu_id,
            ODATA_TYPE: PSU_DATA_TYPE,
            "Id": "PowerSupplyUnit 3",
            "Name": "PowerSupplyUnit 3",
            "Manufacturer": "Delta",
            "Status": { "Health": "OK", "State": "Enabled" }
        }),
    ));

    let supplies = chassis.power_supplies().await?;
    assert!(supplies[0].oem_delta()?.is_none());

    Ok(())
}

#[test]
async fn delta_power_supply_with_null_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_delta_chassis(bmc.clone(), &ids).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    let psu_id = format!("{}/4", ids.psu_collection_id);
    expect_psu_collection(bmc.clone(), &ids, vec![psu_id.clone()]);
    bmc.expect(Expect::get(
        &psu_id,
        json!({
            ODATA_ID: &psu_id,
            ODATA_TYPE: PSU_DATA_TYPE,
            "Id": "PowerSupplyUnit 4",
            "Name": "PowerSupplyUnit 4",
            "Manufacturer": "Delta",
            "Status": { "Health": "OK", "State": "Enabled" },
            "Oem": { "deltaenergysystems": null }
        }),
    ));

    let supplies = chassis.power_supplies().await?;
    assert!(supplies[0].oem_delta()?.is_none());

    Ok(())
}

#[test]
async fn delta_power_supply_oem_multiple_psus() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_delta_chassis(bmc.clone(), &ids).await?;

    expect_power_subsystem(bmc.clone(), &ids);
    let psu0 = format!("{}/3", ids.psu_collection_id);
    let psu1 = format!("{}/4", ids.psu_collection_id);
    expect_psu_collection(bmc.clone(), &ids, vec![psu0.clone(), psu1.clone()]);
    bmc.expect(Expect::get(
        &psu0,
        delta_psu_payload(&psu0, "PowerSupplyUnit 3", Some(true), Some(0)),
    ));
    bmc.expect(Expect::get(
        &psu1,
        delta_psu_payload(&psu1, "PowerSupplyUnit 4", Some(false), Some(0)),
    ));

    let supplies = chassis.power_supplies().await?;
    assert_eq!(supplies.len(), 2);
    assert_eq!(supplies[0].oem_delta()?.unwrap().power(), Some(true));
    assert_eq!(supplies[1].oem_delta()?.unwrap().power(), Some(false));

    Ok(())
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
    let chassis_id = format!("{chassis_collection_id}/chassis");
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

fn delta_chassis_member(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.chassis_id,
        ODATA_TYPE: CHASSIS_DATA_TYPE,
        "Id": "chassis",
        "Name": "chassis",
        "ChassisType": "RackMount",
        "Manufacturer": "DELTA",
        "PowerSubsystem": { ODATA_ID: &ids.power_subsystem_id }
    })
}

fn delta_psu_payload(
    psu_id: &str,
    name: &str,
    power: Option<bool>,
    fan_speed_target: Option<i64>,
) -> Value {
    let mut oem = json!({
        ODATA_ID: format!("{psu_id}#/Oem/deltaenergysystems"),
        ODATA_TYPE: DELTA_PSU_OEM_DATA_TYPE,
    });
    if let Some(power) = power {
        oem["Power"] = json!(power);
    }
    if let Some(fan_speed_target) = fan_speed_target {
        oem["FanSpeedTarget"] = json!(fan_speed_target);
    }
    json!({
        ODATA_ID: psu_id,
        ODATA_TYPE: PSU_DATA_TYPE,
        "Id": name,
        "Name": name,
        "Manufacturer": "Delta",
        "Model": "ECD17020036",
        "Status": { "Health": "OK", "State": "Enabled" },
        "Oem": { "deltaenergysystems": oem }
    })
}

async fn get_delta_chassis(bmc: Arc<Bmc>, ids: &Ids) -> Result<Chassis<Bmc>, Box<dyn StdError>> {
    let service_root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::get(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [delta_chassis_member(ids)]
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
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        anonymous_1_9_service_root(
            &ids.root_id,
            json!({
                "Chassis": { ODATA_ID: &ids.chassis_collection_id }
            }),
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
