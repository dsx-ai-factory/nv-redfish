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

//! Mock-based integration tests for standard Volume and Dell storage operations.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::computer_system::Storage;
use nv_redfish::oem::dell::OperationApplyTime;
use nv_redfish::schema::volume::{LinksCreate, RaidType, VolumeCreate};
use nv_redfish::{Error, ServiceRoot};
use nv_redfish_core::{ModificationResponse, ODataId, Reference, ReferenceLeaf};
use nv_redfish_tests::{assert_empty, assert_task, async_task, Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::{json, Value};

const SYSTEMS_ID: &str = "/redfish/v1/Systems";
const SYSTEM_ID: &str = "/redfish/v1/Systems/system-1";
const STORAGE_COLLECTION_ID: &str = "/redfish/v1/Systems/system-1/Storage";
const STORAGE_ID: &str = "/redfish/v1/Systems/system-1/Storage/controller-1";
const VOLUMES_ID: &str = "/redfish/v1/Systems/system-1/Storage/controller-1/Volumes";
const VOLUME_ID: &str = "/redfish/v1/Systems/system-1/Storage/controller-1/Volumes/volume-1";
const JOB_ID: &str = "/redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/Jobs/JID_000000000001";
const DECOMMISSION_TARGET: &str =
    "/redfish/v1/Systems/system-1/Storage/controller-1/Actions/Oem/DellStorage.ControllerDrivesDecommission";

fn service_root_payload(vendor: Option<&str>) -> Value {
    let root_id = ODataId::service_root();
    json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root service",
        "Vendor": vendor,
        "Systems": { ODATA_ID: SYSTEMS_ID },
        "ProtocolFeaturesSupported": {
            "ExpandQuery": { "NoLinks": true }
        },
        "Links": {
            "Sessions": { ODATA_ID: format!("{root_id}/SessionService/Sessions") }
        }
    })
}

fn system_collection_payload() -> Value {
    json!({
        ODATA_ID: SYSTEMS_ID,
        ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
        "Id": "Systems",
        "Name": "Systems",
        "Members": [{
            ODATA_ID: SYSTEM_ID,
            ODATA_TYPE: "#ComputerSystem.v1_22_0.ComputerSystem",
            "Id": "system-1",
            "Name": "System",
            "SystemType": "Physical",
            "Storage": { ODATA_ID: STORAGE_COLLECTION_ID }
        }]
    })
}

fn storage_collection_payload(oem_actions: Option<Value>) -> Value {
    let mut storage = json!({
        ODATA_ID: STORAGE_ID,
        ODATA_TYPE: "#Storage.v1_17_0.Storage",
        "Id": "controller-1",
        "Name": "Storage controller",
        "Volumes": { ODATA_ID: VOLUMES_ID }
    });
    if let Some(actions) = oem_actions {
        storage["Actions"] = json!({ "Oem": actions });
    }

    json!({
        ODATA_ID: STORAGE_COLLECTION_ID,
        ODATA_TYPE: "#StorageCollection.StorageCollection",
        "Id": "Storage",
        "Name": "Storage",
        "Members": [storage]
    })
}

fn volume_payload(id: &str, name: &str) -> Value {
    json!({
        ODATA_ID: id,
        ODATA_TYPE: "#Volume.v1_10_2.Volume",
        "Id": name,
        "Name": name,
        "Status": {
            "State": "Enabled",
            "Health": "OK"
        },
        "EncryptionTypes": [],
        "Identifiers": [],
        "Operations": [],
        "Links": {},
        "Actions": {}
    })
}

async fn storage(
    bmc: Arc<Bmc>,
    oem_actions: Option<Value>,
) -> Result<Storage<Bmc>, Box<dyn StdError>> {
    storage_from_vendor(bmc, None, oem_actions).await
}

async fn storage_from_vendor(
    bmc: Arc<Bmc>,
    vendor: Option<&str>,
    oem_actions: Option<Value>,
) -> Result<Storage<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    bmc.expect(Expect::get(&root_id, service_root_payload(vendor)));
    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::expand(SYSTEMS_ID, system_collection_payload()));
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
        STORAGE_COLLECTION_ID,
        storage_collection_payload(oem_actions),
    ));
    Ok(system
        .storage_controllers()
        .await?
        .expect("Storage is advertised")
        .into_iter()
        .next()
        .expect("storage controller exists"))
}

async fn volumes(
    bmc: Arc<Bmc>,
) -> Result<nv_redfish::computer_system::VolumeCollection<Bmc>, Box<dyn StdError>> {
    volumes_from_vendor(bmc, None).await
}

async fn volumes_from_vendor(
    bmc: Arc<Bmc>,
    vendor: Option<&str>,
) -> Result<nv_redfish::computer_system::VolumeCollection<Bmc>, Box<dyn StdError>> {
    Ok(storage_from_vendor(bmc, vendor, None)
        .await?
        .volumes()
        .expect("Volumes is advertised"))
}

fn standard_create() -> VolumeCreate {
    VolumeCreate::builder()
        .with_display_name("scratch".to_string())
        .build()
}

fn standard_request_payload() -> Value {
    json!({ "DisplayName": "scratch" })
}

fn dell_create() -> VolumeCreate {
    let drives = ["/redfish/v1/Drives/1", "/redfish/v1/Drives/2"]
        .iter()
        .map(|id| {
            Reference::from(&ReferenceLeaf {
                odata_id: ODataId::from(id.to_string()),
            })
        })
        .collect();
    VolumeCreate::builder()
        .with_name("os".to_string())
        .with_raid_type(RaidType::Raid1)
        .with_links(LinksCreate::builder().with_drives(drives).build())
        .build()
}

fn dell_request_payload() -> Value {
    json!({
        "Name": "os",
        "RAIDType": "RAID1",
        "Links": {
            "Drives": [
                { ODATA_ID: "/redfish/v1/Drives/1" },
                { ODATA_ID: "/redfish/v1/Drives/2" }
            ]
        }
    })
}

fn advertised_decommission_action() -> Value {
    json!({
        "#DellStorage.ControllerDrivesDecommission": {
            "target": DECOMMISSION_TARGET
        }
    })
}

#[tokio::test]
async fn discovers_storage_volumes_and_lists_members() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    let second_volume_id = "/redfish/v1/Systems/system-1/Storage/controller-1/Volumes/volume-2";

    assert_eq!(volumes.odata_id().to_string(), VOLUMES_ID);
    bmc.expect(Expect::get(
        VOLUMES_ID,
        json!({
            ODATA_ID: VOLUMES_ID,
            ODATA_TYPE: "#VolumeCollection.VolumeCollection",
            "Name": "Volumes",
            "Members": [
                volume_payload(VOLUME_ID, "volume-1"),
                { ODATA_ID: second_volume_id }
            ]
        }),
    ));
    bmc.expect(Expect::get(
        second_volume_id,
        volume_payload(second_volume_id, "volume-2"),
    ));

    let members = volumes.members().await?;
    assert_eq!(members.len(), 2);
    assert_eq!(members[0].raw().odata_id.to_string(), VOLUME_ID);
    assert_eq!(members[1].raw().odata_id.to_string(), second_volume_id);
    Ok(())
}

#[tokio::test]
async fn standard_create_uses_embedded_volume_without_get() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    bmc.expect(Expect::create(
        VOLUMES_ID,
        standard_request_payload(),
        volume_payload(VOLUME_ID, "volume-1"),
    ));

    let ModificationResponse::Entity(volume) = volumes.create(&standard_create()).await? else {
        panic!("expected an embedded Volume");
    };
    assert_eq!(volume.raw().odata_id.to_string(), VOLUME_ID);
    Ok(())
}

#[tokio::test]
async fn standard_create_resolves_reference_response() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    bmc.expect(Expect::create(
        VOLUMES_ID,
        standard_request_payload(),
        json!({ ODATA_ID: VOLUME_ID }),
    ));
    bmc.expect(Expect::get(
        VOLUME_ID,
        volume_payload(VOLUME_ID, "volume-1"),
    ));

    let ModificationResponse::Entity(volume) = volumes.create(&standard_create()).await? else {
        panic!("expected a resolved Volume");
    };
    assert_eq!(volume.raw().odata_id.to_string(), VOLUME_ID);
    Ok(())
}

#[tokio::test]
async fn dell_create_returns_job_as_task() -> Result<(), Box<dyn StdError>> {
    for response in [json!({ ODATA_ID: JOB_ID }), job_payload()] {
        let bmc = Arc::new(Bmc::default());
        let volumes = volumes_from_vendor(bmc.clone(), Some("Dell")).await?;
        bmc.expect(Expect::create(
            VOLUMES_ID,
            standard_request_payload(),
            response,
        ));

        let ModificationResponse::Task(task) = volumes.create(&standard_create()).await? else {
            panic!("expected the iDRAC job as a task");
        };
        assert_eq!(task.location.0.to_string(), JOB_ID);
    }
    Ok(())
}

#[tokio::test]
async fn dell_create_fetches_returned_volume() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes_from_vendor(bmc.clone(), Some("Dell")).await?;
    bmc.expect(Expect::create(
        VOLUMES_ID,
        standard_request_payload(),
        json!({ ODATA_ID: VOLUME_ID }),
    ));
    bmc.expect(Expect::get(
        VOLUME_ID,
        volume_payload(VOLUME_ID, "volume-1"),
    ));

    let ModificationResponse::Entity(volume) = volumes.create(&standard_create()).await? else {
        panic!("expected the created Volume");
    };
    assert_eq!(volume.raw().odata_id.to_string(), VOLUME_ID);
    Ok(())
}

#[tokio::test]
async fn standard_create_preserves_task() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    let task_id = "/redfish/v1/TaskService/Tasks/41";
    bmc.expect(Expect::create_task(
        VOLUMES_ID,
        standard_request_payload(),
        async_task(task_id, 5),
    ));

    assert_task(volumes.create(&standard_create()).await?, task_id, 5);
    Ok(())
}

#[tokio::test]
async fn standard_create_preserves_empty() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    bmc.expect(Expect::create_empty(VOLUMES_ID, standard_request_payload()));

    assert_empty(volumes.create(&standard_create()).await?);
    Ok(())
}

#[tokio::test]
async fn dell_create_posts_raid_payload_and_uses_embedded_volume_without_get(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    bmc.expect(Expect::create(
        VOLUMES_ID,
        dell_request_payload(),
        volume_payload(VOLUME_ID, "volume-1"),
    ));

    let ModificationResponse::Entity(volume) = volumes.create(&dell_create()).await? else {
        panic!("expected an embedded Volume");
    };
    assert_eq!(volume.raw().odata_id.to_string(), VOLUME_ID);
    Ok(())
}

#[tokio::test]
async fn dell_create_resolves_reference_response() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    bmc.expect(Expect::create(
        VOLUMES_ID,
        dell_request_payload(),
        json!({ ODATA_ID: VOLUME_ID }),
    ));
    bmc.expect(Expect::get(
        VOLUME_ID,
        volume_payload(VOLUME_ID, "volume-1"),
    ));

    let ModificationResponse::Entity(volume) = volumes.create(&dell_create()).await? else {
        panic!("expected a resolved Volume");
    };
    assert_eq!(volume.raw().odata_id.to_string(), VOLUME_ID);
    Ok(())
}

#[tokio::test]
async fn dell_create_preserves_task() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    let task_id = "/redfish/v1/TaskService/Tasks/42";
    bmc.expect(Expect::create_task(
        VOLUMES_ID,
        dell_request_payload(),
        async_task(task_id, 6),
    ));

    assert_task(volumes.create(&dell_create()).await?, task_id, 6);
    Ok(())
}

#[tokio::test]
async fn dell_create_preserves_empty() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let volumes = volumes(bmc.clone()).await?;
    bmc.expect(Expect::create_empty(VOLUMES_ID, dell_request_payload()));

    assert_empty(volumes.create(&dell_create()).await?);
    Ok(())
}

#[tokio::test]
async fn decommission_serializes_supported_apply_times() -> Result<(), Box<dyn StdError>> {
    for (apply_time, expected) in [
        (OperationApplyTime::Immediate, "Immediate"),
        (OperationApplyTime::OnReset, "OnReset"),
    ] {
        let bmc = Arc::new(Bmc::default());
        let storage = storage(bmc.clone(), Some(advertised_decommission_action())).await?;
        bmc.expect(Expect::action_empty(
            DECOMMISSION_TARGET,
            json!({ "@Redfish.OperationApplyTime": expected }),
        ));

        let actions = storage
            .oem_dell_actions()?
            .expect("Dell OEM actions are advertised");
        assert_empty(
            actions
                .decommission_controller_drives(Some(apply_time))
                .await?,
        );
    }
    Ok(())
}

#[tokio::test]
async fn decommission_omits_unspecified_apply_time() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let storage = storage(bmc.clone(), Some(advertised_decommission_action())).await?;
    bmc.expect(Expect::action_empty(DECOMMISSION_TARGET, json!({})));

    let actions = storage
        .oem_dell_actions()?
        .expect("Dell OEM actions are advertised");
    assert_empty(actions.decommission_controller_drives(None).await?);
    Ok(())
}

#[tokio::test]
async fn decommission_returns_job_as_task() -> Result<(), Box<dyn StdError>> {
    for response in [json!({ ODATA_ID: JOB_ID }), job_payload()] {
        let bmc = Arc::new(Bmc::default());
        let storage = storage(bmc.clone(), Some(advertised_decommission_action())).await?;
        bmc.expect(Expect::action(DECOMMISSION_TARGET, json!({}), response));

        let actions = storage
            .oem_dell_actions()?
            .expect("Dell OEM actions are advertised");
        let ModificationResponse::Task(task) = actions.decommission_controller_drives(None).await?
        else {
            panic!("expected the iDRAC job as a task");
        };
        assert_eq!(task.location.0.to_string(), JOB_ID);
    }
    Ok(())
}

/// Job body iDRAC10 (1.30.30.52) returned with a 201 when it scheduled a
/// configuration job.
fn job_payload() -> Value {
    json!({
        "@odata.context": "/redfish/v1/$metadata#DellJob.DellJob",
        "@odata.etag": "W/\"gen-4\"",
        ODATA_ID: JOB_ID,
        ODATA_TYPE: "#DellJob.v1_8_0.DellJob",
        "ActualRunningStartTime": null,
        "ActualRunningStopTime": null,
        "CompletionTime": null,
        "Description": "Job Instance",
        "EndTime": "TIME_NA",
        "Id": "JID_000000000001",
        "JobState": "ReadyForExecution",
        "JobType": "BIOSConfiguration",
        "Message": "Configuration changes committed",
        "MessageArgs": [],
        "MessageArgs@odata.count": 0,
        "MessageId": "PR19",
        "Name": "Configure: BIOS.Setup.1-1",
        "PercentComplete": 0,
        "StartTime": "2026-09-30T14:43:55",
        "TargetSettingsURI": null,
        "@Message.ExtendedInfo": [
            {
                "MessageId": "Base.1.18.Created",
                "MessageArgs": [],
                "RelatedProperties": [],
                "Message": "The resource was created successfully.",
                "Severity": "OK",
                "Resolution": "None."
            },
            {
                "MessageId": "IDRAC.2.16.SYS414",
                "MessageArgs": [],
                "RelatedProperties": [],
                "Message": "A new resource is successfully created.",
                "Severity": "Informational",
                "Resolution": "No response action is required."
            }
        ]
    })
}

#[tokio::test]
async fn decommission_reports_missing_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let storage = storage(bmc, Some(json!({}))).await?;
    let actions = storage
        .oem_dell_actions()?
        .expect("Dell OEM actions object is advertised");

    assert!(matches!(
        actions.decommission_controller_drives(None).await,
        Err(Error::ActionNotAvailable)
    ));
    Ok(())
}
