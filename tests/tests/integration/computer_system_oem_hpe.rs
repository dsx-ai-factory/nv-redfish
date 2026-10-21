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

//! Integration tests for HPE ComputerSystem OEM actions.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::oem::hpe::HpeSystemResetType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::{ModificationResponse, ODataId};
use nv_redfish_tests::{Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::{json, Value};
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_23_0.ComputerSystem";
const HPE_SYSTEM_DATA_TYPE: &str = "#HpeComputerSystemExt.v2_12_0.HpeComputerSystemExt";

#[test]
async fn system_reset_uses_nested_advertised_hpe_target() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let target = format!("{}/odd-target/SystemReset/", ids.system_id);
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                ODATA_TYPE: HPE_SYSTEM_DATA_TYPE,
                "Actions": {
                    "#HpeComputerSystemExt.SystemReset": {
                        "target": &target
                    }
                }
            })),
        ),
    )
    .await?;
    let actions = system
        .oem_hpe_actions()?
        .ok_or("expected HPE ComputerSystem extension")?;

    bmc.expect(Expect::action(
        &target,
        json!({ "ResetType": "ColdBoot" }),
        json!(null),
    ));
    assert!(matches!(
        actions.system_reset(HpeSystemResetType::ColdBoot).await?,
        ModificationResponse::Entity(())
    ));

    bmc.expect(Expect::action(
        &target,
        json!({ "ResetType": "AuxCycle" }),
        json!(null),
    ));
    assert!(matches!(
        actions.aux_power_cycle().await?,
        ModificationResponse::Entity(())
    ));
    Ok(())
}

#[test]
async fn system_reset_requires_nested_action_advertisement() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc,
        &ids,
        system_payload(
            &ids,
            Some(json!({
                ODATA_TYPE: HPE_SYSTEM_DATA_TYPE,
                "Actions": {}
            })),
        ),
    )
    .await?;
    let actions = system
        .oem_hpe_actions()?
        .ok_or("expected HPE ComputerSystem extension")?;

    assert!(matches!(
        actions.aux_power_cycle().await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));
    Ok(())
}

#[test]
async fn system_without_hpe_extension_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc, &ids, system_payload(&ids, None)).await?;

    assert!(system.oem_hpe_actions()?.is_none());
    Ok(())
}

async fn get_system(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
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
    let root = ServiceRoot::new(bmc.clone()).await?;
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
    systems
        .members()
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "expected one ComputerSystem".into())
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

fn system_payload(ids: &Ids, hpe: Option<Value>) -> Value {
    let mut payload = json!({
        ODATA_ID: &ids.system_id,
        ODATA_TYPE: SYSTEM_DATA_TYPE,
        "Id": "1",
        "Name": "ComputerSystem",
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    });
    if let Some(hpe) = hpe {
        payload["Oem"] = json!({ "Hpe": hpe });
    }
    payload
}
