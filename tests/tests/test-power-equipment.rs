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

//! Integration tests for standard PowerEquipment and PowerShelves.

use nv_redfish::power_equipment::PowerEquipmentType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use std::error::Error as StdError;
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::test;

const ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const POWER_EQUIPMENT_DATA_TYPE: &str = "#PowerEquipment.v1_2_3.PowerEquipment";
const POWER_DISTRIBUTION_COLLECTION_DATA_TYPE: &str =
    "#PowerDistributionCollection.PowerDistributionCollection";
const POWER_SHELF_DATA_TYPE: &str = "#PowerDistribution.v1_6_0.PowerDistribution";

#[test]
async fn power_equipment_lists_power_shelves() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    bmc.expect(Expect::get(
        &ids.root_id,
        root_payload(
            &ids,
            json!({
                "PowerEquipment": {
                    ODATA_ID: &ids.power_equipment_id,
                },
            }),
        ),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &ids.power_equipment_id,
        power_equipment_payload(
            &ids,
            json!({
                "PowerShelves": {
                    ODATA_ID: &ids.power_shelves_id,
                },
            }),
        ),
    ));
    let power_equipment = service_root
        .power_equipment()
        .await?
        .ok_or_else(|| missing("missing PowerEquipment"))?;
    assert_eq!(
        power_equipment.raw().odata_id.to_string(),
        ids.power_equipment_id
    );

    bmc.expect(Expect::expand(
        &ids.power_shelves_id,
        json!({
            ODATA_ID: &ids.power_shelves_id,
            ODATA_TYPE: POWER_DISTRIBUTION_COLLECTION_DATA_TYPE,
            "Name": "Power Shelves",
            "Members": [{
                ODATA_ID: &ids.power_shelf_id,
            }],
        }),
    ));
    let collection = power_equipment
        .power_shelves()
        .await?
        .ok_or_else(|| missing("missing PowerShelves"))?;

    bmc.expect(Expect::get(
        &ids.power_shelf_id,
        json!({
            ODATA_ID: &ids.power_shelf_id,
            ODATA_TYPE: POWER_SHELF_DATA_TYPE,
            "Id": "1",
            "Name": "Power Shelf 1",
            "EquipmentType": "PowerShelf",
            "Manufacturer": "NVIDIA",
            "Model": "NV-PowerShelf-1",
            "SerialNumber": "PS-12345",
            "PartNumber": "900-12345-0000-000",
        }),
    ));
    let shelves = collection.members().await?;
    assert_eq!(shelves.len(), 1);
    let shelf = shelves
        .first()
        .ok_or_else(|| missing("missing power shelf member"))?;
    let raw = shelf.raw();
    assert_eq!(raw.odata_id.to_string(), ids.power_shelf_id);
    assert_eq!(raw.equipment_type, PowerEquipmentType::PowerShelf);
    assert_eq!(raw.manufacturer, Some(Some("NVIDIA".into())));
    assert_eq!(raw.model, Some(Some("NV-PowerShelf-1".into())));
    assert_eq!(raw.serial_number, Some(Some("PS-12345".into())));
    assert_eq!(raw.part_number, Some(Some("900-12345-0000-000".into())));

    Ok(())
}

#[test]
async fn missing_power_equipment_link_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    bmc.expect(Expect::get(&ids.root_id, root_payload(&ids, json!({}))));
    let service_root = ServiceRoot::new(bmc).await?;

    assert!(service_root.power_equipment().await?.is_none());

    Ok(())
}

#[test]
async fn missing_power_shelves_link_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    bmc.expect(Expect::get(
        &ids.root_id,
        root_payload(
            &ids,
            json!({
                "PowerEquipment": {
                    ODATA_ID: &ids.power_equipment_id,
                },
            }),
        ),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &ids.power_equipment_id,
        power_equipment_payload(&ids, json!({})),
    ));
    let power_equipment = service_root
        .power_equipment()
        .await?
        .ok_or_else(|| missing("missing PowerEquipment"))?;

    assert!(power_equipment.power_shelves().await?.is_none());

    Ok(())
}

struct Ids {
    root_id: ODataId,
    power_equipment_id: String,
    power_shelves_id: String,
    power_shelf_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let power_equipment_id = format!("{root_id}/PowerEquipment");
    let power_shelves_id = format!("{power_equipment_id}/PowerShelves");
    let power_shelf_id = format!("{power_shelves_id}/1");
    Ids {
        root_id,
        power_equipment_id,
        power_shelves_id,
        power_shelf_id,
    }
}

fn root_payload(ids: &Ids, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: &ids.root_id,
        ODATA_TYPE: ROOT_DATA_TYPE,
        "Id": "RootService",
        "Name": "Root Service",
        "RedfishVersion": "1.13.0",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": true,
            },
        },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
            },
        },
    });
    json_merge([&base, &fields])
}

fn power_equipment_payload(ids: &Ids, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: &ids.power_equipment_id,
        ODATA_TYPE: POWER_EQUIPMENT_DATA_TYPE,
        "Id": "PowerEquipment",
        "Name": "Power Equipment",
    });
    json_merge([&base, &fields])
}

fn missing(message: &'static str) -> IoError {
    IoError::new(ErrorKind::NotFound, message)
}
