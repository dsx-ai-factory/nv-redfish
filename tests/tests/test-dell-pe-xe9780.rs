// SPDX-FileCopyrightText: Copyright (c) 2025-2026 MIRANTIS, INC. & AFFILIATES. All rights reserved.
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
//

use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::{Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::json;
use std::{error::Error as StdError, str::FromStr, sync::Arc};
use tokio::test;
use uuid::Uuid;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_15_0.ServiceRoot";

#[test]
async fn pe_xe9780_wrong_uuid_field() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let _ = expect_dell_pe_xe9780_service_root(bmc.clone())?;
    let _ = expect_dell_pe_xe9780_systems_collection(bmc.clone())?;
    let root = ServiceRoot::new(bmc.clone()).await?;
    let chassis_odata_id = format!("{}/Chassis", root_id);
    let chassis_first = format!("{}/Chassis_1", chassis_odata_id);
    let chassis_second = format!("{}/Chassis_2", chassis_odata_id);

    let chassis_collection_json = json!({
         ODATA_ID: chassis_odata_id,
         ODATA_TYPE: "#ChassisCollection.ChassisCollection",
         "Description": "Collection of Chassis",
          "Members": [
            {
              ODATA_ID: chassis_first
            },
            {
              ODATA_ID: chassis_second
            }
          ],
          "Members@odata.count": 2,
          "Name": "Chassis Collection"
    });
    let chassis_first_json_buggy = json!({
         ODATA_ID: chassis_first,
         ODATA_TYPE: "#Chassis.v1_22_0.Chassis",
         "Id": "Chassis_1",
         "Name": "Chassis_1",
         "ChassisType": "Component",
         "UUID": "just_a_string"
    });
    let chassis_second_json = json!({
         ODATA_ID: chassis_second,
         ODATA_TYPE: "#Chassis.v1_22_0.Chassis",
         "Id": "Chassis_2",
         "Name": "Chassis_2",
         "ChassisType": "Component",
         "UUID": "ec710fec-e8e4-497e-819e-8c66fb5ffb91"
    });

    bmc.expect(Expect::get(chassis_odata_id, chassis_collection_json));
    bmc.expect(Expect::get(chassis_first, chassis_first_json_buggy));
    bmc.expect(Expect::get(chassis_second, chassis_second_json));

    let chassis_collection = root.chassis().await?.unwrap();
    let members = chassis_collection.members().await?;

    assert!(members[0].raw().uuid.unwrap().is_none());
    assert_eq!(
        members[1].raw().uuid.unwrap().unwrap(),
        Uuid::from_str("ec710fec-e8e4-497e-819e-8c66fb5ffb91").unwrap()
    );

    Ok(())
}

fn expect_dell_pe_xe9780_service_root(bmc: Arc<Bmc>) -> Result<(), Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    let root_json = json!({
      ODATA_ID: &root_id,
      ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
      "Id": "RootService",
      "Name": "Root Service",
      "Vendor": "Dell",
      "Product": "Integrated Dell Remote Access Controller",
      "Links": {
          "Sessions": {
              ODATA_ID: format!("{}/SessionService/Sessions", &root_id),
          }
       },
      "Chassis": {
        ODATA_ID: format!("{}/Chassis", &root_id),
      },
      "Systems": {
        ODATA_ID: format!("{}/Systems", &root_id),
      },
    });
    bmc.expect(Expect::get(&root_id, &root_json));
    Ok(())
}

fn expect_dell_pe_xe9780_systems_collection(bmc: Arc<Bmc>) -> Result<(), Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    let sys_odata_id = format!("{}/Systems", root_id);
    let hgx_odata_id = format!("{}/HGX_Baseboard_0", &sys_odata_id);
    let embedded_system_odata_id = format!("{}/System.Embedded.1", &sys_odata_id);
    let systems_json = json!({
          ODATA_ID: sys_odata_id,
          ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
          "Description": "Collection of Computer Systems",
          "Members": [
            {
              ODATA_ID: hgx_odata_id
            },
            {
              ODATA_ID: embedded_system_odata_id
            }
          ],
          "Members@odata.count": 2,
          "Name": "Computer System Collection"
    });

    let hgx_json = json!(
        {
          ODATA_ID: hgx_odata_id,
          ODATA_TYPE: "#ComputerSystem.v1_22_0.ComputerSystem",
          "Description": "Computer System",
          "Name": "HGX_Baseboard_0",
          "Id": "HGX_Baseboard_0",
          "Model": "NA"
        }
    );

    let embedded_system_json = json!(
        {
          ODATA_ID: embedded_system_odata_id,
          ODATA_TYPE: "#ComputerSystem.v1_25_0.ComputerSystem",
          "Description": "Computer System",
          "Name": "System",
          "Id": "System.Embedded.1",
          "Model": "PowerEdge XE9780",
        }
    );

    bmc.expect(Expect::get(&sys_odata_id, &systems_json));
    bmc.expect(Expect::get(&hgx_odata_id, &hgx_json));
    bmc.expect(Expect::get(
        &embedded_system_odata_id,
        &embedded_system_json,
    ));

    Ok(())
}
