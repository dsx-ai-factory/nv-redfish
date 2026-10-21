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

//! Integration tests for network adapter port discovery.

use nv_redfish::chassis::NetworkAdapter;
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

const CHASSIS_COLLECTION_DATA_TYPE: &str = "#ChassisCollection.ChassisCollection";
const NETWORK_ADAPTER_COLLECTION_DATA_TYPE: &str =
    "#NetworkAdapterCollection.NetworkAdapterCollection";
const PORT_COLLECTION_DATA_TYPE: &str = "#PortCollection.PortCollection";

#[test]
async fn adapter_ports_expose_standard_and_lenovo_mac_addresses_separately(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let adapter = get_adapter(bmc.clone(), &ids, true).await?;
    bmc.expect(Expect::get(
        &ids.ports_id,
        collection_payload(&ids.ports_id, PORT_COLLECTION_DATA_TYPE, &ids.port_ids),
    ));
    let collection = adapter
        .ports()
        .await?
        .expect("adapter must include a port collection");

    let port_fields = [
        json!({
            "Ethernet": {
                "AssociatedMACAddresses": ["10:00:00:00:00:01", "10:00:00:00:00:02"]
            }
        }),
        json!({
            "Oem": {
                "Lenovo": {
                    ODATA_TYPE: "#LenovoPort.v1_0_0.Port",
                    "PhysicalPortMacAddress": "20:00:00:00:00:01"
                }
            }
        }),
        json!({
            "Ethernet": {
                "AssociatedMACAddresses": ["30:00:00:00:00:01"]
            },
            "Oem": {
                "Lenovo": {
                    ODATA_TYPE: "#LenovoPort.v1_0_0.Port",
                    "PhysicalPortMacAddress": "30:00:00:00:00:02"
                }
            }
        }),
        json!({
            "Ethernet": {
                "AssociatedMACAddresses": []
            },
            "Oem": {
                "Lenovo": {
                    ODATA_TYPE: "#LenovoPort.v1_0_0.Port",
                    "PhysicalPortMacAddress": "40:00:00:00:00:01"
                }
            }
        }),
        json!({}),
    ];
    for (port_id, fields) in ids.port_ids.iter().zip(port_fields) {
        bmc.expect(Expect::get(port_id, port_payload(port_id, fields)));
    }

    let mut standard_addresses = Vec::new();
    let mut lenovo_addresses = Vec::new();
    for port in collection.members().await? {
        standard_addresses.push(
            port.associated_mac_addresses()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
        lenovo_addresses.push(
            port.oem_lenovo()?
                .and_then(|lenovo| {
                    lenovo
                        .physical_port_mac_address()
                        .map(|address| address.to_string())
                })
                .into_iter()
                .collect::<Vec<_>>(),
        );
    }

    assert_eq!(
        standard_addresses,
        [
            vec!["10:00:00:00:00:01", "10:00:00:00:00:02"],
            vec![],
            vec!["30:00:00:00:00:01"],
            vec![],
            vec![],
        ]
    );
    assert_eq!(
        lenovo_addresses,
        [
            vec![],
            vec!["20:00:00:00:00:01"],
            vec!["30:00:00:00:00:02"],
            vec!["40:00:00:00:00:01"],
            vec![],
        ]
    );
    Ok(())
}

#[test]
async fn malformed_lenovo_port_does_not_hide_standard_mac_address() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let adapter = get_adapter(bmc.clone(), &ids, true).await?;
    let port_id = &ids.port_ids[0];
    bmc.expect(Expect::get(
        &ids.ports_id,
        collection_payload(
            &ids.ports_id,
            PORT_COLLECTION_DATA_TYPE,
            std::slice::from_ref(port_id),
        ),
    ));
    let collection = adapter
        .ports()
        .await?
        .expect("adapter must include a port collection");
    bmc.expect(Expect::get(
        port_id,
        port_payload(
            port_id,
            json!({
                "Ethernet": {
                    "AssociatedMACAddresses": ["50:00:00:00:00:01"]
                },
                "Oem": {
                    "Lenovo": {
                        ODATA_TYPE: "#LenovoPort.v1_0_0.Port",
                        "PhysicalPortMacAddress": 42
                    }
                }
            }),
        ),
    ));

    let mut ports = collection.members().await?;
    let port = ports.pop().expect("collection must include a port");
    assert_eq!(
        port.associated_mac_addresses()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        ["50:00:00:00:00:01"]
    );
    assert!(port.oem_lenovo().is_err());
    Ok(())
}

#[test]
async fn member_links_allow_independent_port_fetches() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let adapter = get_adapter(bmc.clone(), &ids, true).await?;
    let port_ids = &ids.port_ids[..3];
    bmc.expect(Expect::get(
        &ids.ports_id,
        collection_payload(&ids.ports_id, PORT_COLLECTION_DATA_TYPE, port_ids),
    ));
    let collection = adapter
        .ports()
        .await?
        .expect("adapter must include a port collection");

    bmc.expect(Expect::get(
        &port_ids[0],
        port_payload(
            &port_ids[0],
            json!({
                "Ethernet": {
                    "AssociatedMACAddresses": ["10:00:00:00:00:01"]
                }
            }),
        ),
    ));
    bmc.expect(Expect::get(&port_ids[1], json!({})));
    bmc.expect(Expect::get(
        &port_ids[2],
        port_payload(
            &port_ids[2],
            json!({
                "Ethernet": {
                    "AssociatedMACAddresses": ["30:00:00:00:00:01"]
                }
            }),
        ),
    ));

    let links = collection.member_links();
    assert_eq!(links.len(), 3);
    assert_eq!(
        links[0]
            .upgrade::<nv_redfish::port::Port<Bmc>>()
            .await?
            .associated_mac_addresses()[0]
            .as_str(),
        "10:00:00:00:00:01"
    );
    assert_eq!(links[1].odata_id().to_string(), port_ids[1]);
    assert!(links[1]
        .upgrade::<nv_redfish::port::Port<Bmc>>()
        .await
        .is_err());
    assert_eq!(
        links[2]
            .upgrade::<nv_redfish::port::Port<Bmc>>()
            .await?
            .associated_mac_addresses()[0]
            .as_str(),
        "30:00:00:00:00:01"
    );

    Ok(())
}

#[test]
async fn adapter_without_ports_link_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let adapter = get_adapter(bmc, &ids, false).await?;

    assert!(adapter.ports().await?.is_none());
    Ok(())
}

async fn get_adapter(
    bmc: Arc<Bmc>,
    ids: &Ids,
    with_ports: bool,
) -> Result<NetworkAdapter<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(&ids.root_id, service_root_payload(ids)));
    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &ids.chassis_collection_id,
        collection_payload(
            &ids.chassis_collection_id,
            CHASSIS_COLLECTION_DATA_TYPE,
            std::slice::from_ref(&ids.chassis_id),
        ),
    ));
    let chassis_collection = root
        .chassis()
        .await?
        .expect("service root must include a chassis collection");

    bmc.expect(Expect::get(&ids.chassis_id, chassis_payload(ids)));
    let mut chassis = chassis_collection.members().await?;
    let chassis = chassis.pop().expect("collection must include a chassis");

    bmc.expect(Expect::get(
        &ids.adapters_id,
        collection_payload(
            &ids.adapters_id,
            NETWORK_ADAPTER_COLLECTION_DATA_TYPE,
            std::slice::from_ref(&ids.adapter_id),
        ),
    ));
    bmc.expect(Expect::get(
        &ids.adapter_id,
        adapter_payload(ids, with_ports),
    ));
    let mut adapters = chassis.network_adapters().await?.unwrap_or_default();
    adapters.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing network adapter").into()
    })
}

fn service_root_payload(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "RootService",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": false
            }
        },
        "Chassis": { ODATA_ID: &ids.chassis_collection_id },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
            }
        },
    })
}

fn collection_payload(collection_id: &str, data_type: &str, member_ids: &[String]) -> Value {
    json!({
        ODATA_ID: collection_id,
        ODATA_TYPE: data_type,
        "Name": "Collection",
        "Members": member_ids
            .iter()
            .map(|id| json!({ ODATA_ID: id }))
            .collect::<Vec<_>>(),
    })
}

fn chassis_payload(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.chassis_id,
        ODATA_TYPE: "#Chassis.v1_23_0.Chassis",
        "Id": "1",
        "Name": "Chassis",
        "ChassisType": "RackMount",
        "NetworkAdapters": { ODATA_ID: &ids.adapters_id },
    })
}

fn adapter_payload(ids: &Ids, with_ports: bool) -> Value {
    let mut payload = json!({
        ODATA_ID: &ids.adapter_id,
        ODATA_TYPE: "#NetworkAdapter.v1_10_0.NetworkAdapter",
        "Id": "1",
        "Name": "Network Adapter",
    });
    if with_ports {
        payload["Ports"] = json!({ ODATA_ID: &ids.ports_id });
    }
    payload
}

fn port_payload(port_id: &str, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: port_id,
        ODATA_TYPE: "#Port.v1_10_0.Port",
        "Id": port_id.rsplit('/').next().unwrap_or_default(),
        "Name": "Port",
    });
    json_merge([&base, &fields])
}

struct Ids {
    root_id: ODataId,
    chassis_collection_id: String,
    chassis_id: String,
    adapters_id: String,
    adapter_id: String,
    ports_id: String,
    port_ids: Vec<String>,
}

impl Ids {
    fn new() -> Self {
        let root_id = ODataId::service_root();
        let chassis_collection_id = format!("{root_id}/Chassis");
        let chassis_id = format!("{chassis_collection_id}/1");
        let adapters_id = format!("{chassis_id}/NetworkAdapters");
        let adapter_id = format!("{adapters_id}/1");
        let ports_id = format!("{adapter_id}/Ports");
        let port_ids = (1..=5).map(|id| format!("{ports_id}/{id}")).collect();
        Self {
            root_id,
            chassis_collection_id,
            chassis_id,
            adapters_id,
            adapter_id,
            ports_id,
            port_ids,
        }
    }
}
