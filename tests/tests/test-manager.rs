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
use nv_redfish::manager::ManagerUpdate;
use nv_redfish::resource::ResetType;
use nv_redfish::schema::manager_network_protocol::ProtocolUpdate;
use nv_redfish::schema::serial_interface::FlowControl;
use nv_redfish::schema::serial_interface::Parity;
use nv_redfish::schema::serial_interface::PinOut;
use nv_redfish::schema::serial_interface::SignalType;
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
const SERIAL_INTERFACE_COLLECTION_DATA_TYPE: &str =
    "#SerialInterfaceCollection.SerialInterfaceCollection";
const SERIAL_INTERFACE_DATA_TYPE: &str = "#SerialInterface.v1_1_8.SerialInterface";

#[test]
async fn filtered_log_count_quirk_is_limited_to_nvidia_platforms() -> Result<(), Box<dyn StdError>>
{
    for (name, root_fields, expected) in [
        ("NVIDIA", json!({ "Vendor": "NVIDIA" }), true),
        (
            "Vera Rubin",
            json!({ "Vendor": "NVIDIA", "Product": "VR NVL72" }),
            true,
        ),
        (
            "NVIDIA GB300",
            json!({ "Vendor": "NVIDIA", "Product": "GB BMC" }),
            true,
        ),
        (
            "NVSwitch",
            json!({ "Vendor": "NVIDIA", "Product": "P3809" }),
            false,
        ),
        (
            "NVIDIA DPU",
            json!({ "Vendor": "Nvidia", "Product": "BlueField-4" }),
            false,
        ),
        ("Dell", json!({ "Vendor": "Dell" }), false),
        ("AMI", json!({ "Vendor": "AMI" }), false),
        ("Wiwynn", json!({ "Vendor": "WIWYNN" }), false),
        ("unknown vendor", json!({ "Vendor": "Unrecognized" }), false),
        (
            "missing vendor",
            json!({ "RedfishVersion": "1.16.0" }),
            false,
        ),
    ] {
        let bmc = Arc::new(Bmc::default());
        let ids = ids();
        let log_services_id = format!("{}/LogServices", ids.manager_id);
        let event_log_id = format!("{log_services_id}/EventLog");

        let member = manager_payload_with_fields(
            &ids,
            json!({
                "LogServices": {
                    ODATA_ID: log_services_id,
                    ODATA_TYPE: "#LogServiceCollection.LogServiceCollection",
                    "Name": "Log services",
                    "Members": [{
                        ODATA_ID: event_log_id,
                        ODATA_TYPE: "#LogService.v1_0_0.LogService",
                        "Id": "EventLog",
                        "Name": "Event log"
                    }]
                }
            }),
        );

        let manager = get_manager_with_root_fields(bmc, &ids, root_fields, member).await?;

        let mut services = manager.log_services().await?.expect("log services");
        let service = services.pop().expect("event log service");

        assert_eq!(
            service.filtered_entries_count_is_unreliable(),
            expected,
            "{name}"
        );
    }

    Ok(())
}

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
async fn network_protocol_accepts_null_ntp_server_entries() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager_with_root_fields(
        bmc.clone(),
        &ids,
        json!({ "Vendor": "Lenovo" }),
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
            "NTP": { "NTPServers": [null, "pool.ntp.org"] },
            "IPMI": { "Port": 623 }
        }),
    ));

    let network = manager.network_protocol().await?.unwrap();
    let raw = network.raw();
    let servers = raw
        .ntp
        .as_ref()
        .unwrap()
        .ntp_servers
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(servers, &[None, Some("pool.ntp.org".to_string())]);
    assert_eq!(raw.ipmi.as_ref().unwrap().port, Some(Some(623)));
    Ok(())
}

#[test]
async fn manager_accepts_null_ntp_servers_in_expanded_link() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Vendor": "Lenovo",
            "Managers": { ODATA_ID: &ids.managers_id },
            "ProtocolFeaturesSupported": { "ExpandQuery": { "NoLinks": true } }
        }),
    )
    .await?;

    bmc.expect(Expect::expand(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [manager_payload_with_fields(
                &ids,
                json!({
                    "NetworkProtocol": {
                        ODATA_ID: &ids.manager_network_protocol_id,
                        ODATA_TYPE: MANAGER_NETWORK_PROTOCOL_DATA_TYPE,
                        "Id": "NetworkProtocol",
                        "Name": "Manager Network Protocol",
                        "NTP": { "NTPServers": [null, "pool.ntp.org"] }
                    }
                }),
            )]
        }),
    ));

    let manager = root
        .managers()
        .await?
        .unwrap()
        .members()
        .await?
        .pop()
        .unwrap();
    let network = manager.network_protocol().await?.unwrap();
    let servers = network
        .raw()
        .ntp
        .as_ref()
        .unwrap()
        .ntp_servers
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(servers, [None, Some("pool.ntp.org".to_string())]);
    Ok(())
}

#[test]
async fn serial_interfaces_read_supermicro_sol_settings() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let serial_interfaces_id = format!("{}/SerialInterfaces", ids.manager_id);
    let serial_interface_id = format!("{serial_interfaces_id}/1");
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(
            &ids,
            json!({ "SerialInterfaces": { ODATA_ID: &serial_interfaces_id } }),
        ),
    )
    .await?;

    bmc.expect(Expect::get(
        &serial_interfaces_id,
        json!({
            ODATA_ID: &serial_interfaces_id,
            ODATA_TYPE: SERIAL_INTERFACE_COLLECTION_DATA_TYPE,
            "Name": "Serial Interface Collection",
            "Members": [{
                ODATA_ID: &serial_interface_id,
                ODATA_TYPE: SERIAL_INTERFACE_DATA_TYPE,
                "Id": "1",
                "Name": "SerialInterface",
                "Description": "Serial over LAN",
                "InterfaceEnabled": true,
                "SignalType": "Rs232",
                "BitRate": "115200",
                "Parity": "None",
                "DataBits": "8",
                "StopBits": "1",
                "FlowControl": "None",
                "ConnectorType": "RJ45",
                "PinOut": "Cyclades"
            }]
        }),
    ));

    let serial = manager
        .serial_interfaces()
        .await?
        .ok_or_else(|| std::io::Error::other("missing serial interfaces"))?
        .members()
        .await?
        .pop()
        .ok_or_else(|| std::io::Error::other("missing serial interface"))?;
    let raw = serial.raw();

    assert_eq!(serial.interface_enabled(), Some(true));
    assert_eq!(raw.signal_type, Some(SignalType::Rs232));
    assert_eq!(raw.bit_rate.as_deref(), Some("115200"));
    assert_eq!(raw.parity, Some(Parity::None));
    assert_eq!(raw.data_bits.as_deref(), Some("8"));
    assert_eq!(raw.stop_bits.as_deref(), Some("1"));
    assert_eq!(raw.flow_control, Some(FlowControl::None));
    assert_eq!(raw.connector_type.as_deref(), Some("RJ45"));
    assert_eq!(raw.pin_out, Some(Some(PinOut::Cyclades)));

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
async fn manager_update_patches_manager_and_refetches() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload(&ids)).await?;

    let update = ManagerUpdate::builder()
        .with_service_identification("rack-7".into())
        .build();
    bmc.expect(Expect::update(
        &ids.manager_id,
        json!({ "ServiceIdentification": "rack-7" }),
        json!({ ODATA_ID: &ids.manager_id }),
    ));
    bmc.expect(Expect::get(
        &ids.manager_id,
        manager_payload_with_fields(&ids, json!({ "ServiceIdentification": "rack-7" })),
    ));
    let ModificationResponse::Entity(updated) = manager.update(&update).await? else {
        return Err(std::io::Error::other("expected manager entity response").into());
    };
    assert_eq!(
        updated
            .raw()
            .service_identification
            .as_ref()
            .and_then(Option::as_deref),
        Some("rack-7")
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
async fn liteon_reset_to_defaults_sends_reset_to_defaults_type() -> Result<(), Box<dyn StdError>> {
    // Detected from the manager's Manufacturer (service root without Vendor)
    // or from a Lite-On service root Vendor.
    for (root_fields, manager_fields) in [
        (
            json!({}),
            json!({ "Manufacturer": "LITE-ON TECHNOLOGY CORP." }),
        ),
        (
            json!({ "Vendor": "LITE-ON TECHNOLOGY CORP.", "RedfishVersion": "1.15.0" }),
            json!({}),
        ),
    ] {
        let bmc = Arc::new(Bmc::default());
        let ids = ids();
        let action_target = format!("{}/Actions/Manager.ResetToDefaults", ids.manager_id);
        let manager = get_manager_with_root_fields(
            bmc.clone(),
            &ids,
            root_fields,
            manager_payload_with_fields(
                &ids,
                json_merge([
                    &redfish_action_payload("Manager.ResetToDefaults", &action_target),
                    &manager_fields,
                ]),
            ),
        )
        .await?;

        bmc.expect(Expect::action(
            &action_target,
            json!({ "ResetToDefaultsType": "ResetAll" }),
            json!(null),
        ));

        assert!(matches!(
            manager
                .reset_to_defaults(ManagerResetToDefaultsType::ResetAll)
                .await?,
            ModificationResponse::Entity(())
        ));
    }

    Ok(())
}

#[test]
async fn delta_reset_to_defaults_sends_reset_type() -> Result<(), Box<dyn StdError>> {
    // Delta shares the vendor-less Redfish 1.9.0 service root with Lite-On.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let action_target = format!("{}/Actions/Manager.ResetToDefaults", ids.manager_id);
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload_with_fields(
            &ids,
            json_merge([
                &redfish_action_payload("Manager.ResetToDefaults", &action_target),
                &json!({ "Manufacturer": "Delta" }),
            ]),
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
    get_manager_with_root_fields(bmc, ids, json!({}), member).await
}

async fn get_manager_with_root_fields(
    bmc: Arc<Bmc>,
    ids: &Ids,
    root_fields: Value,
    member: Value,
) -> Result<Manager<Bmc>, Box<dyn StdError>> {
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        ids,
        json_merge([
            &json!({ "Managers": { ODATA_ID: &ids.managers_id } }),
            &root_fields,
        ]),
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
