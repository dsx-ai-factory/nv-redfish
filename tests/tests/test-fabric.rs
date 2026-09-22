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

//! Integration tests for Fabric and Switch traversal.

use nv_redfish::fabric::Fabric;
use nv_redfish::fabric::Protocol;
use nv_redfish::oem::nvidia::switch::FabricManagerState;
use nv_redfish::oem::nvidia::switch::SwitchIsolationMode;
use nv_redfish::resource::Health;
use nv_redfish::resource::PowerState;
use nv_redfish::resource::State;
use nv_redfish::switch::Switch;
use nv_redfish::ResourceProvidesStatus as _;
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

const FABRIC_COLLECTION_DATA_TYPE: &str = "#FabricCollection.FabricCollection";
const FABRIC_DATA_TYPE: &str = "#Fabric.v1_3_0.Fabric";
const SWITCH_COLLECTION_DATA_TYPE: &str = "#SwitchCollection.SwitchCollection";
const SWITCH_DATA_TYPE: &str = "#Switch.v1_9_0.Switch";
const PORT_COLLECTION_DATA_TYPE: &str = "#PortCollection.PortCollection";
const PORT_DATA_TYPE: &str = "#Port.v1_10_0.Port";
const NVIDIA_FABRIC_DATA_TYPE: &str = "#NvidiaFabric.v1_0_0.NvidiaFabric";
const NVIDIA_SWITCH_DATA_TYPE: &str = "#NvidiaSwitch.v1_5_0.NvidiaSwitch";

#[test]
async fn traverses_fabrics_switches_and_ports() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let root = get_root(bmc.clone(), &ids, true).await?;

    bmc.expect(Expect::get(
        &ids.fabrics_id,
        collection_payload(
            &ids.fabrics_id,
            FABRIC_COLLECTION_DATA_TYPE,
            std::slice::from_ref(&ids.fabric_id),
        ),
    ));
    let fabrics = root
        .fabrics()
        .await?
        .expect("service root must include fabrics");

    bmc.expect(Expect::get(
        &ids.fabric_id,
        fabric_payload(
            &ids,
            json!({
                "Switches": { ODATA_ID: &ids.switches_id },
                "FabricType": "NVLink",
                "MaxZones": 16,
                "Status": { "State": "Enabled", "Health": "OK" },
            }),
        ),
    ));
    let mut fabrics = fabrics.members().await?;
    let fabric = fabrics.pop().expect("collection must include a fabric");
    assert_eq!(fabric.raw().id, "NVLinkFabric_0");
    assert_eq!(fabric.fabric_type(), Some(Protocol::NvLink));
    assert_eq!(fabric.max_zones(), Some(16));
    let status = fabric.status().expect("fabric must include status");
    assert_eq!(status.state, Some(State::Enabled));
    assert_eq!(status.health, Some(Health::Ok));

    bmc.expect(Expect::get(
        &ids.switches_id,
        collection_payload(
            &ids.switches_id,
            SWITCH_COLLECTION_DATA_TYPE,
            &ids.switch_ids,
        ),
    ));
    let switches = fabric
        .switches()
        .await?
        .expect("fabric must include switches");

    bmc.expect(Expect::get(
        &ids.switch_ids[0],
        switch_payload(
            &ids.switch_ids[0],
            json!({
                "SwitchType": "NVLink",
                "SupportedProtocols": ["NVLink"],
                "Manufacturer": "NVIDIA",
                "Model": "NVSwitch",
                "PartNumber": "PN-0",
                "SerialNumber": "SN-0",
                "SKU": "SKU-0",
                "FirmwareVersion": "1.2.3",
                "PowerState": "On",
                "Enabled": true,
                "IsManaged": true,
                "TotalSwitchWidth": 72,
                "CurrentBandwidthGbps": 100.5,
                "MaxBandwidthGbps": 200,
                "Status": { "State": "Enabled", "Health": "Warning" },
                "Ports": { ODATA_ID: &ids.ports_id },
            }),
        ),
    ));
    bmc.expect(Expect::get(
        &ids.switch_ids[1],
        switch_payload(&ids.switch_ids[1], json!({})),
    ));
    let switches = switches.members().await?;
    assert_eq!(switches.len(), 2);

    let switch = &switches[0];
    let hw = switch.hardware_id();
    assert_eq!(
        hw.manufacturer.map(|v| v.to_string()),
        Some("NVIDIA".into())
    );
    assert_eq!(hw.model.map(|v| v.to_string()), Some("NVSwitch".into()));
    assert_eq!(hw.part_number.map(|v| v.to_string()), Some("PN-0".into()));
    assert_eq!(hw.serial_number.map(|v| v.to_string()), Some("SN-0".into()));
    assert_eq!(switch.sku().map(|v| v.to_string()), Some("SKU-0".into()));
    assert_eq!(
        switch.firmware_version().map(|v| v.to_string()),
        Some("1.2.3".into())
    );
    assert_eq!(switch.switch_type(), Some(Protocol::NvLink));
    assert_eq!(switch.supported_protocols(), vec![Protocol::NvLink]);
    assert_eq!(switch.power_state(), Some(PowerState::On));
    assert_eq!(switch.enabled(), Some(true));
    assert_eq!(switch.is_managed(), Some(true));
    assert_eq!(switch.total_switch_width(), Some(72));
    assert_eq!(switch.current_bandwidth_gbps(), Some(100.5));
    assert_eq!(switch.max_bandwidth_gbps(), Some(200.0));
    assert_eq!(
        switch.status().and_then(|s| s.health),
        Some(Health::Warning)
    );

    // A switch reporting nothing beyond the required properties
    // reads as absent everywhere rather than failing.
    let bare = &switches[1];
    assert!(bare.hardware_id().manufacturer.is_none());
    assert!(bare.switch_type().is_none());
    assert!(bare.supported_protocols().is_empty());
    assert!(bare.status().is_none());

    bmc.expect(Expect::get(
        &ids.ports_id,
        collection_payload(
            &ids.ports_id,
            PORT_COLLECTION_DATA_TYPE,
            std::slice::from_ref(&ids.port_id),
        ),
    ));
    let ports = switch.ports().await?.expect("switch must include ports");
    bmc.expect(Expect::get(
        &ids.port_id,
        json!({
            ODATA_ID: &ids.port_id,
            ODATA_TYPE: PORT_DATA_TYPE,
            "Id": "NVLink_0",
            "Name": "NVLink Port 0",
        }),
    ));
    let ports = ports.members().await?;
    assert_eq!(ports.len(), 1);
    assert_eq!(ports[0].raw().id, "NVLink_0");

    Ok(())
}

#[test]
async fn member_links_allow_independent_switch_fetches() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let fabric = get_fabric(
        bmc.clone(),
        &ids,
        json!({ "Switches": { ODATA_ID: &ids.switches_id } }),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.switches_id,
        collection_payload(
            &ids.switches_id,
            SWITCH_COLLECTION_DATA_TYPE,
            &ids.switch_ids,
        ),
    ));
    let switches = fabric
        .switches()
        .await?
        .expect("fabric must include switches");
    let links = switches.member_links();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0].odata_id().to_string(), ids.switch_ids[0]);

    // A malformed switch fails its own fetch without affecting others.
    bmc.expect(Expect::get(&ids.switch_ids[0], json!({})));
    assert!(links[0].upgrade::<Switch<Bmc>>().await.is_err());

    bmc.expect(Expect::get(
        &ids.switch_ids[1],
        switch_payload(&ids.switch_ids[1], json!({ "Model": "NVSwitch" })),
    ));
    let switch = links[1].upgrade::<Switch<Bmc>>().await?;
    assert_eq!(
        switch.hardware_id().model.map(|v| v.to_string()),
        Some("NVSwitch".into())
    );

    Ok(())
}

#[test]
async fn fabric_member_links_upgrade() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let root = get_root(bmc.clone(), &ids, true).await?;

    bmc.expect(Expect::get(
        &ids.fabrics_id,
        collection_payload(
            &ids.fabrics_id,
            FABRIC_COLLECTION_DATA_TYPE,
            std::slice::from_ref(&ids.fabric_id),
        ),
    ));
    let links = root
        .fabrics()
        .await?
        .expect("service root must include fabrics")
        .member_links();
    assert_eq!(links.len(), 1);

    bmc.expect(Expect::get(
        &ids.fabric_id,
        fabric_payload(&ids, json!({ "FabricType": "PCIe" })),
    ));
    let fabric = links[0].upgrade::<Fabric<Bmc>>().await?;
    assert_eq!(fabric.fabric_type(), Some(Protocol::Pcie));

    Ok(())
}

#[test]
async fn missing_links_return_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();

    let root = get_root(bmc.clone(), &ids, false).await?;
    assert!(root.fabrics().await?.is_none());

    let fabric = get_fabric(bmc.clone(), &ids, json!({})).await?;
    assert!(fabric.switches().await?.is_none());
    assert!(fabric.oem_nvidia()?.is_none());

    let switch = get_switch(bmc.clone(), &ids, json!({})).await?;
    assert!(switch.ports().await?.is_none());
    assert!(switch.oem_nvidia()?.is_none());

    Ok(())
}

#[test]
async fn empty_switch_collection_has_no_members() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let fabric = get_fabric(
        bmc.clone(),
        &ids,
        json!({ "Switches": { ODATA_ID: &ids.switches_id } }),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.switches_id,
        collection_payload(&ids.switches_id, SWITCH_COLLECTION_DATA_TYPE, &[]),
    ));
    let switches = fabric
        .switches()
        .await?
        .expect("fabric must include switches");
    assert!(switches.members().await?.is_empty());
    assert!(switches.member_links().is_empty());

    Ok(())
}

#[test]
async fn malformed_switch_response_is_an_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let fabric = get_fabric(
        bmc.clone(),
        &ids,
        json!({ "Switches": { ODATA_ID: &ids.switches_id } }),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.switches_id,
        collection_payload(
            &ids.switches_id,
            SWITCH_COLLECTION_DATA_TYPE,
            &ids.switch_ids[..1],
        ),
    ));
    let switches = fabric
        .switches()
        .await?
        .expect("fabric must include switches");

    bmc.expect(Expect::get(
        &ids.switch_ids[0],
        switch_payload(&ids.switch_ids[0], json!({ "TotalSwitchWidth": "wide" })),
    ));
    assert!(switches.members().await.is_err());

    Ok(())
}

#[test]
async fn malformed_fabric_collection_is_an_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let root = get_root(bmc.clone(), &ids, true).await?;

    bmc.expect(Expect::get(
        &ids.fabrics_id,
        json!({
            ODATA_ID: &ids.fabrics_id,
            ODATA_TYPE: FABRIC_COLLECTION_DATA_TYPE,
            "Name": "Fabric Collection",
            "Members": { ODATA_ID: &ids.fabric_id },
        }),
    ));
    assert!(root.fabrics().await.is_err());

    Ok(())
}

#[test]
async fn oem_nvidia_switch_properties() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let switch = get_switch(
        bmc.clone(),
        &ids,
        json!({
            "Oem": {
                "Nvidia": {
                    ODATA_TYPE: NVIDIA_SWITCH_DATA_TYPE,
                    "DeviceId": "0x22a3",
                    "VendorId": "0x10de",
                    "PCIeReferenceClockEnabled": true,
                    "PPCIeModeEnabled": false,
                    "SwitchIsolationMode": "SwitchCommunicationDisabled",
                    "FabricManager": {
                        "State": "Configured",
                        "DurationSinceLastRestartSeconds": 3600,
                    },
                }
            }
        }),
    )
    .await?;

    let oem = switch
        .oem_nvidia()?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(
        oem.device_id().map(|v| v.to_string()),
        Some("0x22a3".into())
    );
    assert_eq!(
        oem.vendor_id().map(|v| v.to_string()),
        Some("0x10de".into())
    );
    assert_eq!(oem.pcie_reference_clock_enabled(), Some(true));
    assert_eq!(oem.ppcie_mode_enabled(), Some(false));
    assert_eq!(
        oem.switch_isolation_mode(),
        Some(SwitchIsolationMode::SwitchCommunicationDisabled)
    );
    assert_eq!(
        oem.fabric_manager_state(),
        Some(FabricManagerState::Configured)
    );
    assert_eq!(
        oem.fabric_manager()
            .and_then(|fm| fm.duration_since_last_restart_seconds)
            .flatten(),
        Some(3600)
    );

    Ok(())
}

#[test]
async fn oem_nvidia_fabric_properties() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let push_uri = format!("{}/Actions/Oem/SwitchConfigPush", ids.fabric_id);
    let fabric = get_fabric(
        bmc,
        &ids,
        json!({
            "Oem": {
                "Nvidia": {
                    ODATA_TYPE: NVIDIA_FABRIC_DATA_TYPE,
                    "SwitchConfigPushURI": &push_uri,
                }
            }
        }),
    )
    .await?;

    let oem = fabric
        .oem_nvidia()?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(oem.switch_config_push_uri(), Some(push_uri.as_str()));

    Ok(())
}

#[test]
async fn null_nvidia_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let switch = get_switch(bmc, &ids, json!({ "Oem": { "Nvidia": Value::Null } })).await?;

    assert!(switch.oem_nvidia()?.is_none());

    Ok(())
}

#[test]
async fn malformed_nvidia_oem_does_not_hide_standard_properties() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = Ids::new();
    let switch = get_switch(
        bmc,
        &ids,
        json!({
            "Model": "NVSwitch",
            "Oem": {
                "Nvidia": {
                    ODATA_TYPE: NVIDIA_SWITCH_DATA_TYPE,
                    "PCIeReferenceClockEnabled": "yes",
                }
            }
        }),
    )
    .await?;

    assert_eq!(
        switch.hardware_id().model.map(|v| v.to_string()),
        Some("NVSwitch".into())
    );
    assert!(switch.oem_nvidia().is_err());

    Ok(())
}

async fn get_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    with_fabrics: bool,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        service_root_payload(ids, with_fabrics),
    ));
    Ok(ServiceRoot::new(bmc).await?)
}

async fn get_fabric(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<Fabric<Bmc>, Box<dyn StdError>> {
    let root = get_root(bmc.clone(), ids, true).await?;
    bmc.expect(Expect::get(
        &ids.fabrics_id,
        collection_payload(
            &ids.fabrics_id,
            FABRIC_COLLECTION_DATA_TYPE,
            std::slice::from_ref(&ids.fabric_id),
        ),
    ));
    let fabrics = root
        .fabrics()
        .await?
        .expect("service root must include fabrics");
    bmc.expect(Expect::get(&ids.fabric_id, fabric_payload(ids, fields)));
    fabrics
        .members()
        .await?
        .pop()
        .ok_or_else(|| std::io::Error::other("missing fabric").into())
}

async fn get_switch(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<Switch<Bmc>, Box<dyn StdError>> {
    let fabric = get_fabric(
        bmc.clone(),
        ids,
        json!({ "Switches": { ODATA_ID: &ids.switches_id } }),
    )
    .await?;
    bmc.expect(Expect::get(
        &ids.switches_id,
        collection_payload(
            &ids.switches_id,
            SWITCH_COLLECTION_DATA_TYPE,
            &ids.switch_ids[..1],
        ),
    ));
    let switches = fabric
        .switches()
        .await?
        .ok_or_else(|| std::io::Error::other("missing switches"))?;
    bmc.expect(Expect::get(
        &ids.switch_ids[0],
        switch_payload(&ids.switch_ids[0], fields),
    ));
    switches
        .members()
        .await?
        .pop()
        .ok_or_else(|| std::io::Error::other("missing switch").into())
}

fn service_root_payload(ids: &Ids, with_fabrics: bool) -> Value {
    let mut payload = json!({
        ODATA_ID: &ids.root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "RootService",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": false
            }
        },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
            }
        },
    });
    if with_fabrics {
        payload["Fabrics"] = json!({ ODATA_ID: &ids.fabrics_id });
    }
    payload
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

fn fabric_payload(ids: &Ids, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: &ids.fabric_id,
        ODATA_TYPE: FABRIC_DATA_TYPE,
        "Id": "NVLinkFabric_0",
        "Name": "NVLink Fabric",
    });
    json_merge([&base, &fields])
}

fn switch_payload(switch_id: &str, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: switch_id,
        ODATA_TYPE: SWITCH_DATA_TYPE,
        "Id": switch_id.rsplit('/').next().unwrap_or_default(),
        "Name": "Switch",
    });
    json_merge([&base, &fields])
}

struct Ids {
    root_id: ODataId,
    fabrics_id: String,
    fabric_id: String,
    switches_id: String,
    switch_ids: Vec<String>,
    ports_id: String,
    port_id: String,
}

impl Ids {
    fn new() -> Self {
        let root_id = ODataId::service_root();
        let fabrics_id = format!("{root_id}/Fabrics");
        let fabric_id = format!("{fabrics_id}/NVLinkFabric_0");
        let switches_id = format!("{fabric_id}/Switches");
        let switch_ids: Vec<String> = (0..2)
            .map(|id| format!("{switches_id}/NVSwitch_{id}"))
            .collect();
        let ports_id = format!("{}/Ports", switch_ids[0]);
        let port_id = format!("{ports_id}/NVLink_0");
        Self {
            root_id,
            fabrics_id,
            fabric_id,
            switches_id,
            switch_ids,
            ports_id,
            port_id,
        }
    }
}
