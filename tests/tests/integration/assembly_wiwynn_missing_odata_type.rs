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
//! Integration tests for WIWYNN assembly payloads missing `@odata.type`.

use nv_redfish::chassis::Chassis;
use nv_redfish::hardware_id::Model;
use nv_redfish::hardware_id::PartNumber;
use nv_redfish::hardware_id::SerialNumber;
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
use std::sync::Arc;
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const CHASSIS_COLLECTION_DATA_TYPE: &str = "#ChassisCollection.ChassisCollection";
const CHASSIS_DATA_TYPE: &str = "#Chassis.v1_22_0.Chassis";
const ASSEMBLY_DATA_TYPE: &str = "#Assembly.v1_3_0.Assembly";
const ASSEMBLY_MEMBER_DATA_TYPE: &str = "#Assembly.v1_5_1.AssemblyData";
const DUMMY_SERIAL: &str = "B8111801000851800AAAY0ZZ";

#[test]
async fn wiwynn_assembly_without_member_odata_type_is_supported() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = test_ids();
    let chassis = get_chassis(bmc.clone(), &ids, "WIWYNN").await?;

    bmc.expect(Expect::expand(
        &ids.assembly_id,
        assembly_payload(&ids, false, DUMMY_SERIAL),
    ));
    let assembly = chassis.assembly().await?.unwrap();
    let members = assembly.assemblies().await?;
    assert_eq!(members.len(), 1);

    let hw = members[0].hardware_id();
    assert_eq!(hw.model, Some(Model::new("GB200 NVL")));
    assert_eq!(hw.part_number, Some(PartNumber::new("B81.11801.0008")));
    assert_eq!(hw.serial_number, Some(SerialNumber::new(DUMMY_SERIAL)));

    Ok(())
}

#[test]
async fn wiwynn_assembly_with_member_odata_type_still_supported() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = test_ids();
    let chassis = get_chassis(bmc.clone(), &ids, "WIWYNN").await?;

    bmc.expect(Expect::expand(
        &ids.assembly_id,
        assembly_payload(&ids, true, DUMMY_SERIAL),
    ));
    let assembly = chassis.assembly().await?.unwrap();
    let members = assembly.assemblies().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

async fn get_chassis(
    bmc: Arc<Bmc>,
    ids: &TestIds,
    vendor: &str,
) -> Result<Chassis<Bmc>, Box<dyn StdError>> {
    let service_root = expect_service_root(bmc.clone(), ids, vendor).await?;
    bmc.expect(Expect::expand(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [chassis_member(ids)]
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
    ids: &TestIds,
    vendor: &str,
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
            "Vendor": vendor,
            "Chassis": { ODATA_ID: &ids.chassis_collection_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct TestIds {
    root_id: ODataId,
    chassis_collection_id: String,
    chassis_id: String,
    assembly_id: String,
    assembly_member_id: String,
}

fn test_ids() -> TestIds {
    let root_id = ODataId::service_root();
    let chassis_collection_id = format!("{root_id}/Chassis");
    let chassis_id = format!("{chassis_collection_id}/Chassis_0");
    let assembly_id = format!("{chassis_id}/Assembly");
    let assembly_member_id = format!("{assembly_id}#/Assemblies/0");
    TestIds {
        root_id,
        chassis_collection_id,
        chassis_id,
        assembly_id,
        assembly_member_id,
    }
}

fn chassis_member(ids: &TestIds) -> Value {
    json!({
        ODATA_ID: &ids.chassis_id,
        ODATA_TYPE: CHASSIS_DATA_TYPE,
        "Id": "Chassis_0",
        "Name": "Chassis_0",
        "ChassisType": "RackMount",
        "Assembly": {
            ODATA_ID: &ids.assembly_id
        },
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    })
}

fn assembly_payload(ids: &TestIds, include_member_odata_type: bool, serial_number: &str) -> Value {
    let member_base = json!({
        ODATA_ID: &ids.assembly_member_id,
        "Location": {
            "PartLocation": {
                "LocationType": "Embedded"
            }
        },
        "MemberId": "0",
        "Model": "GB200 NVL",
        "Name": "PDB Chassis FRU Assembly0",
        "PartNumber": "B81.11801.0008",
        "SerialNumber": serial_number,
        "Vendor": "NVIDIA"
    });
    let member = if include_member_odata_type {
        json_merge([
            &member_base,
            &json!({
                ODATA_TYPE: ASSEMBLY_MEMBER_DATA_TYPE
            }),
        ])
    } else {
        member_base
    };

    json!({
        ODATA_ID: &ids.assembly_id,
        ODATA_TYPE: ASSEMBLY_DATA_TYPE,
        "Id": "Assembly",
        "Name": "Assembly data for Chassis_0",
        "Assemblies": [member]
    })
}
