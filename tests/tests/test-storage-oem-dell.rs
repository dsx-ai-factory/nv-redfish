// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Dell actions advertised by Storage resources.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::oem::dell::DellVolumeCreate;
use nv_redfish::schema::settings::ApplyTime;
use nv_redfish::schema::volume::{RaidType, VolumeCreate};
use nv_redfish::ServiceRoot;
use nv_redfish_core::{ModificationResponse, ODataId};
use nv_redfish_tests::{assert_empty, assert_task, async_task, Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::json;

#[tokio::test]
async fn storage_invokes_advertised_dell_decommission_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/system-1");
    let storage_collection_id = "/redfish/v1/vendor/storage";
    let storage_id = "/redfish/v1/vendor/storage/controller-1";
    let volumes_id = "/redfish/v1/vendor/storage/controller-1/volumes";
    let action_target = "/redfish/v1/vendor/actions/decommission";

    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
            "Id": "RootService",
            "Name": "Root service",
            "Systems": { ODATA_ID: &systems_id },
            "ProtocolFeaturesSupported": {
                "ExpandQuery": { "NoLinks": true }
            },
            "Links": {
                "Sessions": { ODATA_ID: format!("{root_id}/SessionService/Sessions") }
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;
    bmc.expect(Expect::expand(
        &systems_id,
        json!({
            ODATA_ID: &systems_id,
            ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
            "Id": "Systems",
            "Name": "Systems",
            "Members": [{
                ODATA_ID: &system_id,
                ODATA_TYPE: "#ComputerSystem.v1_22_0.ComputerSystem",
                "Id": "system-1",
                "Name": "System",
                "SystemType": "Physical",
                "Storage": { ODATA_ID: storage_collection_id }
            }]
        }),
    ));
    let system = root
        .systems()
        .await?
        .expect("Systems is advertised")
        .members()
        .await?
        .into_iter()
        .next()
        .expect("system exists");
    bmc.expect(Expect::expand(
        storage_collection_id,
        json!({
            ODATA_ID: storage_collection_id,
            ODATA_TYPE: "#StorageCollection.StorageCollection",
            "Id": "Storage",
            "Name": "Storage",
            "Members": [{
                ODATA_ID: storage_id,
                ODATA_TYPE: "#Storage.v1_17_0.Storage",
                "Id": "controller-1",
                "Name": "Storage controller",
                "Volumes": { ODATA_ID: volumes_id },
                "Actions": {
                    "Oem": {
                        "#DellStorage.ControllerDrivesDecommission": {
                            "target": action_target
                        }
                    }
                }
            }]
        }),
    ));
    let storage = system
        .storage_controllers()
        .await?
        .expect("Storage is advertised")
        .into_iter()
        .next()
        .expect("storage controller exists");

    bmc.expect(Expect::action(
        action_target,
        json!({ "@Redfish.OperationApplyTime": "Immediate" }),
        json!(null),
    ));
    let actions = storage
        .oem_dell_actions()?
        .expect("Dell action is advertised");
    assert!(matches!(
        actions
            .decommission_controller_drives(ApplyTime::Immediate)
            .await?,
        ModificationResponse::Entity(())
    ));

    let volumes = storage.volumes().expect("Volumes is advertised");
    assert_eq!(volumes.odata_id().to_string(), volumes_id);
    bmc.expect(Expect::get(
        volumes_id,
        json!({
            ODATA_ID: volumes_id,
            ODATA_TYPE: "#VolumeCollection.VolumeCollection",
            "Name": "Volumes",
            "Members": []
        }),
    ));
    assert!(volumes.members().await?.is_empty());

    let standard = VolumeCreate::builder()
        .with_display_name("scratch".to_string())
        .build();
    bmc.expect(Expect::create_empty(
        volumes_id,
        json!({ "DisplayName": "scratch" }),
    ));
    assert_empty(volumes.create(&standard).await?);

    let drives = vec![
        ODataId::from("/redfish/v1/Drives/1".to_string()),
        ODataId::from("/redfish/v1/Drives/2".to_string()),
    ];
    let dell = DellVolumeCreate::new("os".to_string(), RaidType::Raid1, drives);
    let task_id = "/redfish/v1/JobService/Jobs/JID_43";
    bmc.expect(Expect::create_task(
        volumes_id,
        json!({
            "Name": "os",
            "RAIDType": "RAID1",
            "Links": {
                "Drives": [
                    { ODATA_ID: "/redfish/v1/Drives/1" },
                    { ODATA_ID: "/redfish/v1/Drives/2" }
                ]
            }
        }),
        async_task(task_id, 5),
    ));
    assert_task(volumes.oem_dell().create(&dell).await?, task_id, 5);

    Ok(())
}
