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

//! Integration tests for HPE ManagerNetworkProtocol OEM support.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::manager::ManagerNetworkProtocol;
use nv_redfish::ServiceRoot;
use nv_redfish_core::{ModificationResponse, ODataId};
use nv_redfish_tests::{Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::{json, Value};
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const MANAGER_COLLECTION_DATA_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_DATA_TYPE: &str = "#Manager.v1_16_0.Manager";
const NETWORK_PROTOCOL_DATA_TYPE: &str = "#ManagerNetworkProtocol.v1_5_0.ManagerNetworkProtocol";
const HPE_NETWORK_PROTOCOL_DATA_TYPE: &str =
    "#HpeiLOManagerNetworkService.v2_3_0.HpeiLOManagerNetworkService";

#[test]
async fn hpe_kcs_is_typed_and_updatable() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let network =
        get_network_protocol(bmc.clone(), &ids, network_payload(&ids, Some(json!(false)))).await?;
    let hpe = network
        .oem_hpe()?
        .ok_or("HPE network-protocol extension missing")?;
    assert_eq!(hpe.kcs_enabled(), Some(false));

    bmc.expect(Expect::update(
        &ids.network_protocol_id,
        json!({ "Oem": { "Hpe": { "KcsEnabled": true } } }),
        network_payload(&ids, Some(json!(true))),
    ));
    let ModificationResponse::Entity(updated) = hpe
        .set_kcs_enabled(true)
        .await?
        .ok_or("KcsEnabled missing")?
    else {
        return Err("expected updated ManagerNetworkProtocol".into());
    };
    assert_eq!(
        updated.oem_hpe()?.and_then(|hpe| hpe.kcs_enabled()),
        Some(true)
    );
    Ok(())
}

#[test]
async fn hpe_kcs_update_requires_advertised_property() -> Result<(), Box<dyn StdError>> {
    for value in [None, Some(Value::Null)] {
        let bmc = Arc::new(Bmc::default());
        let ids = ids();
        let network = get_network_protocol(bmc, &ids, network_payload(&ids, value)).await?;
        let hpe = network
            .oem_hpe()?
            .ok_or("HPE network-protocol extension missing")?;
        assert_eq!(hpe.kcs_enabled(), None);
        assert!(hpe.set_kcs_enabled(true).await?.is_none());
    }
    Ok(())
}

#[test]
async fn network_protocol_without_hpe_extension_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let network = get_network_protocol(
        bmc,
        &ids,
        json!({
            ODATA_ID: &ids.network_protocol_id,
            ODATA_TYPE: NETWORK_PROTOCOL_DATA_TYPE,
            "Id": "NetworkProtocol",
            "Name": "Manager Network Protocol",
            "Oem": {}
        }),
    )
    .await?;

    assert!(network.oem_hpe()?.is_none());
    Ok(())
}

async fn get_network_protocol(
    bmc: Arc<Bmc>,
    ids: &Ids,
    payload: Value,
) -> Result<ManagerNetworkProtocol<Bmc>, Box<dyn StdError>> {
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
            "Managers": { ODATA_ID: &ids.managers_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id)
                }
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;
    bmc.expect(Expect::expand(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Name": "Manager Collection",
            "Members": [{
                ODATA_ID: &ids.manager_id,
                ODATA_TYPE: MANAGER_DATA_TYPE,
                "Id": "1",
                "Name": "Manager",
                "ManagerType": "BMC",
                "NetworkProtocol": { ODATA_ID: &ids.network_protocol_id }
            }]
        }),
    ));
    let manager = root
        .managers()
        .await?
        .ok_or("expected Managers")?
        .members()
        .await?
        .into_iter()
        .next()
        .ok_or("expected one Manager")?;
    bmc.expect(Expect::get(&ids.network_protocol_id, payload));
    manager
        .network_protocol()
        .await?
        .ok_or_else(|| "expected ManagerNetworkProtocol".into())
}

struct Ids {
    root_id: ODataId,
    managers_id: String,
    manager_id: String,
    network_protocol_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    let manager_id = format!("{managers_id}/1");
    let network_protocol_id = format!("{manager_id}/NetworkProtocol");
    Ids {
        root_id,
        managers_id,
        manager_id,
        network_protocol_id,
    }
}

fn network_payload(ids: &Ids, kcs_enabled: Option<Value>) -> Value {
    let mut hpe = json!({ ODATA_TYPE: HPE_NETWORK_PROTOCOL_DATA_TYPE });
    if let Some(kcs_enabled) = kcs_enabled {
        hpe["KcsEnabled"] = kcs_enabled;
    }
    json!({
        ODATA_ID: &ids.network_protocol_id,
        ODATA_TYPE: NETWORK_PROTOCOL_DATA_TYPE,
        "@odata.etag": "W/\"network-protocol-etag\"",
        "Id": "NetworkProtocol",
        "Name": "Manager Network Protocol",
        "Oem": { "Hpe": hpe }
    })
}
