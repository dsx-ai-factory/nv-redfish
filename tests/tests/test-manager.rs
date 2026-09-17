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
//! Integration tests for Manager collection behavior.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::ethernet_interface::EthernetInterfaceUpdate;
use nv_redfish::host_interface::HostInterfaceUpdate;
use nv_redfish::manager::Manager;
use nv_redfish::manager::ManagerNetworkProtocolUpdate;
use nv_redfish::manager::ManagerResetToDefaultsType;
use nv_redfish::resource::ResetType;
use nv_redfish::schema::manager_network_protocol::ProtocolUpdate;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::ami_viking_service_root;
use nv_redfish_tests::anonymous_1_9_service_root;
use nv_redfish_tests::assert_empty;
use nv_redfish_tests::assert_task;
use nv_redfish_tests::async_task;
use nv_redfish_tests::expect_redfish_reset_action;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::redfish_action_payload;
use nv_redfish_tests::redfish_empty_actions_payload;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;

use serde_json::json;
use serde_json::Value;
use tokio::test;

const MANAGER_COLLECTION_DATA_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_DATA_TYPE: &str = "#Manager.v1_16_0.Manager";
const MANAGER_NETWORK_PROTOCOL_DATA_TYPE: &str =
    "#ManagerNetworkProtocol.v1_5_0.ManagerNetworkProtocol";
const ETHERNET_INTERFACE_COLLECTION_DATA_TYPE: &str =
    "#EthernetInterfaceCollection.EthernetInterfaceCollection";
const ETHERNET_INTERFACE_DATA_TYPE: &str = "#EthernetInterface.v1_10_0.EthernetInterface";
const HOST_INTERFACE_COLLECTION_DATA_TYPE: &str =
    "#HostInterfaceCollection.HostInterfaceCollection";
const HOST_INTERFACE_DATA_TYPE: &str = "#HostInterface.v1_3_0.HostInterface";

#[test]
async fn network_protocol_returns_none_when_link_is_absent() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc, &ids, manager_payload(&ids)).await?;

    assert!(manager.network_protocol().await?.is_none());

    Ok(())
}

#[test]
async fn network_protocol_fetches_linked_resource() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(
            &ids,
            json!({ "NetworkProtocol": { ODATA_ID: &ids.manager_network_protocol_id } }),
        ),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.manager_network_protocol_id,
        json!({
            ODATA_ID: &ids.manager_network_protocol_id,
            ODATA_TYPE: MANAGER_NETWORK_PROTOCOL_DATA_TYPE,
            "Id": "NetworkProtocol",
            "Name": "Manager Network Protocol",
            "IPMI": {
                "ProtocolEnabled": true,
                "Port": 1623
            }
        }),
    ));

    let network_protocol = manager
        .network_protocol()
        .await?
        .ok_or_else(|| std::io::Error::other("missing manager network protocol"))?;
    let raw = network_protocol.raw();
    let ipmi = raw
        .ipmi
        .as_ref()
        .ok_or_else(|| std::io::Error::other("missing IPMI protocol"))?;

    assert_eq!(ipmi.protocol_enabled, Some(Some(true)));
    assert_eq!(ipmi.port, Some(Some(1623)));

    Ok(())
}

#[test]
async fn typed_updates_use_resource_uris_and_map_responses() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let ethernet_interfaces_id = format!("{}/EthernetInterfaces", ids.manager_id);
    let ethernet_interface_id = format!("{ethernet_interfaces_id}/1");
    let host_interfaces_id = format!("{}/HostInterfaces", ids.manager_id);
    let host_interface_id = format!("{host_interfaces_id}/1");
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(
            &ids,
            json!({
                "NetworkProtocol": { ODATA_ID: &ids.manager_network_protocol_id },
                "EthernetInterfaces": { ODATA_ID: &ethernet_interfaces_id },
                "HostInterfaces": { ODATA_ID: &host_interfaces_id }
            }),
        ),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.manager_network_protocol_id,
        json!({
            ODATA_ID: &ids.manager_network_protocol_id,
            ODATA_TYPE: MANAGER_NETWORK_PROTOCOL_DATA_TYPE,
            "Id": "NetworkProtocol",
            "Name": "Manager Network Protocol",
            "IPMI": { "ProtocolEnabled": true, "Port": 623 }
        }),
    ));
    let network_protocol = manager
        .network_protocol()
        .await?
        .ok_or_else(|| std::io::Error::other("missing network protocol"))?;
    let protocol = ProtocolUpdate::builder()
        .with_protocol_enabled(false)
        .with_port(6623)
        .build();
    let network_update = ManagerNetworkProtocolUpdate::builder()
        .with_ipmi(protocol)
        .build();
    bmc.expect(Expect::update(
        &ids.manager_network_protocol_id,
        json!({ "IPMI": { "ProtocolEnabled": false, "Port": 6623 } }),
        json!({ ODATA_ID: &ids.manager_network_protocol_id }),
    ));
    bmc.expect(Expect::get(
        &ids.manager_network_protocol_id,
        json!({
            ODATA_ID: &ids.manager_network_protocol_id,
            ODATA_TYPE: MANAGER_NETWORK_PROTOCOL_DATA_TYPE,
            "Id": "NetworkProtocol",
            "Name": "Manager Network Protocol",
            "IPMI": { "ProtocolEnabled": false, "Port": 6623 }
        }),
    ));
    let ModificationResponse::Entity(updated_network) =
        network_protocol.update(&network_update).await?
    else {
        return Err(std::io::Error::other("expected network entity response").into());
    };
    assert_eq!(
        updated_network
            .raw()
            .ipmi
            .as_ref()
            .and_then(|ipmi| ipmi.port),
        Some(Some(6623))
    );
    let task_id = "/redfish/v1/TaskService/Tasks/network-update";
    bmc.expect(Expect::update_task(
        &ids.manager_network_protocol_id,
        serde_json::to_value(&network_update)?,
        async_task(task_id, 3),
    ));
    assert_task(updated_network.update(&network_update).await?, task_id, 3);
    bmc.expect(Expect::update_empty(
        &ids.manager_network_protocol_id,
        serde_json::to_value(&network_update)?,
    ));
    assert_empty(updated_network.update(&network_update).await?);

    bmc.expect(Expect::get(
        &ethernet_interfaces_id,
        json!({
            ODATA_ID: &ethernet_interfaces_id,
            ODATA_TYPE: ETHERNET_INTERFACE_COLLECTION_DATA_TYPE,
            "Name": "Ethernet Interfaces",
            "Members": [{
                ODATA_ID: &ethernet_interface_id,
                ODATA_TYPE: ETHERNET_INTERFACE_DATA_TYPE,
                "Id": "1",
                "Name": "Ethernet Interface",
                "InterfaceEnabled": true,
                "MTUSize": 1500
            }]
        }),
    ));
    let ethernet = manager
        .ethernet_interfaces()
        .await?
        .ok_or_else(|| std::io::Error::other("missing ethernet interfaces"))?
        .members()
        .await?
        .pop()
        .ok_or_else(|| std::io::Error::other("missing ethernet interface"))?;
    let ethernet_update = EthernetInterfaceUpdate::builder()
        .with_interface_enabled(false)
        .with_mtu_size(9000)
        .build();
    bmc.expect(Expect::update(
        &ethernet_interface_id,
        json!({ "InterfaceEnabled": false, "MTUSize": 9000 }),
        json!({
            ODATA_ID: &ethernet_interface_id,
            ODATA_TYPE: ETHERNET_INTERFACE_DATA_TYPE,
            "Id": "1",
            "Name": "Ethernet Interface",
            "InterfaceEnabled": false,
            "MTUSize": 9000
        }),
    ));
    let ModificationResponse::Entity(updated_ethernet) = ethernet.update(&ethernet_update).await?
    else {
        return Err(std::io::Error::other("expected ethernet entity response").into());
    };
    assert_eq!(updated_ethernet.interface_enabled(), Some(false));
    assert_eq!(updated_ethernet.raw().mtu_size, Some(Some(9000)));

    bmc.expect(Expect::get(
        &host_interfaces_id,
        json!({
            ODATA_ID: &host_interfaces_id,
            ODATA_TYPE: HOST_INTERFACE_COLLECTION_DATA_TYPE,
            "Name": "Host Interfaces",
            "Members": [{
                ODATA_ID: &host_interface_id,
                ODATA_TYPE: HOST_INTERFACE_DATA_TYPE,
                "Id": "1",
                "Name": "Host Interface",
                "InterfaceEnabled": true
            }]
        }),
    ));
    let host = manager
        .host_interfaces()
        .await?
        .ok_or_else(|| std::io::Error::other("missing host interfaces"))?
        .members()
        .await?
        .pop()
        .ok_or_else(|| std::io::Error::other("missing host interface"))?;
    let host_update = HostInterfaceUpdate::builder()
        .with_interface_enabled(false)
        .with_firmware_auth_role_id("Operator".into())
        .build();
    bmc.expect(Expect::update(
        &host_interface_id,
        json!({ "InterfaceEnabled": false, "FirmwareAuthRoleId": "Operator" }),
        json!({
            ODATA_ID: &host_interface_id,
            ODATA_TYPE: HOST_INTERFACE_DATA_TYPE,
            "Id": "1",
            "Name": "Host Interface",
            "InterfaceEnabled": false,
            "FirmwareAuthRoleId": "Operator"
        }),
    ));
    let ModificationResponse::Entity(updated_host) = host.update(&host_update).await? else {
        return Err(std::io::Error::other("expected host entity response").into());
    };
    assert_eq!(updated_host.interface_enabled(), Some(false));
    assert_eq!(
        updated_host.raw().firmware_auth_role_id.as_deref(),
        Some("Operator")
    );

    Ok(())
}

#[test]
async fn reset_invokes_manager_reset_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let action_target = format!("{}/Actions/Manager.Reset", ids.manager_id);
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(
            &ids,
            redfish_action_payload("Manager.Reset", &action_target),
        ),
    )
    .await?;

    expect_redfish_reset_action(&bmc, &action_target, Some("ForceRestart"));

    assert!(matches!(
        manager.reset(Some(ResetType::ForceRestart)).await?,
        ModificationResponse::Entity(())
    ));

    expect_redfish_reset_action(&bmc, &action_target, None);

    assert!(matches!(
        manager.reset(None).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn reset_to_defaults_invokes_manager_reset_to_defaults_action(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let action_target = format!("{}/Actions/Manager.ResetToDefaults", ids.manager_id);
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(
            &ids,
            redfish_action_payload("Manager.ResetToDefaults", &action_target),
        ),
    )
    .await?;

    expect_redfish_reset_action(&bmc, &action_target, Some("ResetAll"));

    assert!(matches!(
        manager
            .reset_to_defaults(ManagerResetToDefaultsType::ResetAll)
            .await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn reset_helpers_return_action_not_available_when_manager_actions_are_absent(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(&ids, redfish_empty_actions_payload()),
    )
    .await?;

    assert!(matches!(
        manager.reset(Some(ResetType::ForceRestart)).await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));
    assert!(matches!(
        manager
            .reset_to_defaults(ManagerResetToDefaultsType::ResetAll)
            .await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));

    Ok(())
}

#[test]
async fn ami_viking_missing_root_managers_nav_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    bmc.expect(Expect::get(
        &ids.root_id,
        ami_viking_service_root(&ids.root_id, json!({})),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [manager_payload(&ids)]
        }),
    ));

    let collection = root.managers().await?.unwrap();
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn anonymous_1_9_0_wrong_manager_status_state_workaround() -> Result<(), Box<dyn StdError>> {
    // Platform under test: Liteon powershelf class (anonymous Redfish 1.9.0 root).
    // Quirk under test: invalid Manager.Status.State="Standby".
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Managers": { ODATA_ID: &ids.managers_id }
        }),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [{ ODATA_ID: &ids.manager_id }]
        }),
    ));

    let collection = root.managers().await?.unwrap();
    bmc.expect(Expect::get(
        &ids.manager_id,
        manager_payload_with_state(&ids, "Standby"),
    ));
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn viking_with_garbage_in_managers() -> Result<(), Box<dyn StdError>> {
    // Viking returns garbage entries in Managers collection that should be filtered out.
    // Valid entries: /BMC, /HGX_BMC_0, /HGX_FabricManager_0
    // Garbage entries: /BMC/NodeManager, /HGX_BMC_0/Actions/Manager.Reset, /HGX_BMC_0/ResetActionInfo
    let bmc = Arc::new(Bmc::default());
    let ids = ids();

    bmc.expect(Expect::get(
        &ids.root_id,
        ami_viking_service_root(
            &ids.root_id,
            json!({
                "Managers": { ODATA_ID: &ids.managers_id }
            }),
        ),
    ));

    let service_root = ServiceRoot::new(bmc.clone()).await?;

    // Valid manager IDs
    let bmc_id = format!("{}/BMC", ids.managers_id);
    let hgx_bmc_id = format!("{}/HGX_BMC_0", ids.managers_id);
    let fabric_mgr_id = format!("{}/HGX_FabricManager_0", ids.managers_id);

    // Garbage IDs that should be filtered out
    let node_manager_id = format!("{}/BMC/NodeManager", ids.managers_id);
    let reset_action_id = format!("{}/HGX_BMC_0/Actions/Manager.Reset", ids.managers_id);
    let reset_action_info_id = format!("{}/HGX_BMC_0/ResetActionInfo", ids.managers_id);

    bmc.expect(Expect::get(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [
                { ODATA_ID: &bmc_id },
                { ODATA_ID: &node_manager_id },
                { ODATA_ID: &reset_action_id },
                { ODATA_ID: &hgx_bmc_id },
                { ODATA_ID: &reset_action_info_id },
                { ODATA_ID: &fabric_mgr_id },
            ]
        }),
    ));

    let collection = service_root.managers().await?.unwrap();

    // Expect GET requests only for valid managers
    bmc.expect(Expect::get(&bmc_id, manager_payload_with_id(&bmc_id)));
    bmc.expect(Expect::get(
        &hgx_bmc_id,
        manager_payload_with_id(&hgx_bmc_id),
    ));
    bmc.expect(Expect::get(
        &fabric_mgr_id,
        manager_payload_with_id(&fabric_mgr_id),
    ));

    let members = collection.members().await?;

    // Should only have 3 valid managers, not the garbage entries
    assert_eq!(members.len(), 3);

    let member_ids: Vec<_> = members
        .iter()
        .map(|m| m.raw().odata_id.to_string())
        .collect();
    assert!(member_ids.contains(&bmc_id));
    assert!(member_ids.contains(&hgx_bmc_id));
    assert!(member_ids.contains(&fabric_mgr_id));
    assert!(!member_ids.contains(&node_manager_id));
    assert!(!member_ids.contains(&reset_action_id));
    assert!(!member_ids.contains(&reset_action_info_id));

    Ok(())
}

struct Ids {
    root_id: ODataId,
    managers_id: String,
    manager_id: String,
    manager_network_protocol_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    let manager_id = format!("{managers_id}/1");
    let manager_network_protocol_id = format!("{manager_id}/NetworkProtocol");
    Ids {
        root_id,
        managers_id,
        manager_id,
        manager_network_protocol_id,
    }
}

fn manager_payload(ids: &Ids) -> serde_json::Value {
    manager_payload_with_state(ids, "Enabled")
}

fn manager_payload_with_state(ids: &Ids, state: &str) -> Value {
    manager_payload_with_fields(ids, json!({ "Status": { "State": state } }))
}

fn manager_payload_with_fields(ids: &Ids, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "Status": { "State": "Enabled" }
    });
    json_merge([&base, &fields])
}

async fn expect_anonymous_1_9_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        anonymous_1_9_service_root(&ids.root_id, fields),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

fn manager_payload_with_id(id: &str) -> Value {
    let name = id.rsplit('/').next().unwrap_or("Manager");
    json!({
        ODATA_ID: id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": name,
        "Name": name,
        "Status": { "State": "Enabled" }
    })
}

async fn get_manager(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
) -> Result<Manager<Bmc>, Box<dyn StdError>> {
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        ids,
        json!({
            "Managers": { ODATA_ID: &ids.managers_id }
        }),
    )
    .await?;
    bmc.expect(Expect::get(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [member]
        }),
    ));

    let collection = root.managers().await?.unwrap();
    let mut members = collection.members().await?;
    members.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing manager").into()
    })
}
