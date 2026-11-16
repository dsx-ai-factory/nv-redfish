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

//! Mock-based integration tests for storage controller access.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::computer_system::Storage;
use nv_redfish::{Error, ServiceRoot};
use nv_redfish_core::ODataId;
use nv_redfish_tests::{Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::{json, Value};

const SYSTEMS_ID: &str = "/redfish/v1/Systems";
const SYSTEM_ID: &str = "/redfish/v1/Systems/Self";
const STORAGE_COLLECTION_ID: &str = "/redfish/v1/Systems/Self/Storage";
const STORAGE_ID: &str = "/redfish/v1/Systems/Self/Storage/1";
const DRIVE_ID: &str = "/redfish/v1/Systems/Self/Storage/1/Drives/0";
const CONTROLLERS_ID: &str = "/redfish/v1/Systems/Self/Storage/1/Controllers";
const CONTROLLER_ID: &str = "/redfish/v1/Systems/Self/Storage/1/Controllers/0";
const MICRON_MODEL: &str = "Micron_7450_MTFDKBA960TFR";
const INTEL_MODEL: &str = "INTEL SSDPF2KE016T1O";

fn service_root_payload() -> Value {
    let root_id = ODataId::service_root();
    json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root service",
        "Vendor": "AMI",
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
            "Id": "Self",
            "Name": "System",
            "SystemType": "Physical",
            "Storage": { ODATA_ID: STORAGE_COLLECTION_ID }
        }]
    })
}

/// Storage resource shaped like the one a Lenovo ThinkSystem HS350X V3
/// (AMI BMC) returns: controller firmware is only in the embedded
/// `StorageControllers` array.
fn ami_storage_payload() -> Value {
    json!({
        ODATA_ID: STORAGE_ID,
        ODATA_TYPE: "#Storage.v1_12_0.Storage",
        "Id": "1",
        "Name": "NVMe Storage",
        "Drives": [{ ODATA_ID: DRIVE_ID }],
        "StorageControllers": [
            {
                ODATA_ID: format!("{STORAGE_ID}#/StorageControllers/0"),
                "MemberId": "StorageUnit_0",
                "Model": MICRON_MODEL,
                "FirmwareVersion": "E2MU116",
                "Status": { "State": "Enabled", "Health": "OK" }
            },
            {
                ODATA_ID: format!("{STORAGE_ID}#/StorageControllers/1"),
                "MemberId": "StorageUnit_1",
                "Model": INTEL_MODEL,
                "FirmwareVersion": "9CV10420",
                "Status": { "State": "Enabled", "Health": "OK" }
            }
        ]
    })
}

async fn storage(bmc: Arc<Bmc>, storage: Value) -> Result<Storage<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(ODataId::service_root(), service_root_payload()));
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
        json!({
            ODATA_ID: STORAGE_COLLECTION_ID,
            ODATA_TYPE: "#StorageCollection.StorageCollection",
            "Name": "Storage",
            "Members": [storage]
        }),
    ));
    Ok(system
        .storage_controllers()
        .await?
        .expect("Storage is advertised")
        .into_iter()
        .next()
        .expect("storage exists"))
}

#[tokio::test]
async fn embedded_storage_controllers_are_typed() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let storage = storage(bmc.clone(), ami_storage_payload()).await?;

    let controllers = storage
        .storage_controllers()
        .await?
        .expect("StorageControllers is embedded");
    assert_eq!(controllers.len(), 2);
    assert_eq!(controllers[0].member_id, "StorageUnit_0");
    assert_eq!(controllers[0].model, Some(Some(MICRON_MODEL.into())));
    assert_eq!(
        controllers[0].firmware_version,
        Some(Some("E2MU116".into()))
    );
    assert!(controllers[0].status.is_some());
    assert_eq!(controllers[1].model, Some(Some(INTEL_MODEL.into())));
    assert_eq!(
        controllers[1].firmware_version,
        Some(Some("9CV10420".into()))
    );
    Ok(())
}

#[tokio::test]
async fn embedded_controller_firmware_matches_drive_model() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let storage = storage(bmc.clone(), ami_storage_payload()).await?;
    bmc.expect(Expect::get(
        DRIVE_ID,
        json!({
            ODATA_ID: DRIVE_ID,
            ODATA_TYPE: "#Drive.v1_13_0.Drive",
            "Id": "0",
            "Name": "Drive 0",
            "Model": MICRON_MODEL,
            "Revision": "NVMe Spec v1.4",
            "Protocol": "NVMe"
        }),
    ));

    let drive = storage
        .drives()
        .await?
        .expect("Drives is advertised")
        .into_iter()
        .next()
        .expect("drive exists")
        .raw();
    let firmware = storage
        .storage_controllers()
        .await?
        .expect("StorageControllers is embedded")
        .into_iter()
        .find(|c| c.model == drive.model)
        .and_then(|c| c.firmware_version.clone().flatten());
    assert_eq!(firmware.as_deref(), Some("E2MU116"));
    Ok(())
}

#[tokio::test]
async fn referenced_storage_controller_is_an_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let entry_id = format!("{STORAGE_ID}#/StorageControllers/0");
    let storage = storage(
        bmc.clone(),
        json!({
            ODATA_ID: STORAGE_ID,
            ODATA_TYPE: "#Storage.v1_12_0.Storage",
            "Id": "1",
            "Name": "NVMe Storage",
            "StorageControllers": [{ ODATA_ID: &entry_id }]
        }),
    )
    .await?;

    // No GET is expected: the fragment reference must not become a request.
    match storage.storage_controllers().await {
        Err(Error::StorageControllerNotExpanded(id)) => assert_eq!(id.to_string(), entry_id),
        other => panic!(
            "expected StorageControllerNotExpanded, got {:?}",
            other.err()
        ),
    }
    Ok(())
}

#[tokio::test]
async fn controllers_fetches_collection_members() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let storage = storage(
        bmc.clone(),
        json!({
            ODATA_ID: STORAGE_ID,
            ODATA_TYPE: "#Storage.v1_15_0.Storage",
            "Id": "1",
            "Name": "NVMe Storage",
            "Controllers": { ODATA_ID: CONTROLLERS_ID }
        }),
    )
    .await?;
    bmc.expect(Expect::get(
        CONTROLLERS_ID,
        json!({
            ODATA_ID: CONTROLLERS_ID,
            ODATA_TYPE: "#StorageControllerCollection.StorageControllerCollection",
            "Name": "Storage Controllers",
            "Members": [{ ODATA_ID: CONTROLLER_ID }]
        }),
    ));
    bmc.expect(Expect::get(
        CONTROLLER_ID,
        json!({
            ODATA_ID: CONTROLLER_ID,
            ODATA_TYPE: "#StorageController.v1_7_0.StorageController",
            "Id": "0",
            "Name": "Storage Controller 0",
            "Model": INTEL_MODEL,
            "FirmwareVersion": "9CV10420",
            "Status": { "State": "Enabled", "Health": "OK" }
        }),
    ));

    let controllers = storage
        .controllers()
        .await?
        .expect("Controllers is advertised");
    assert_eq!(controllers.len(), 1);
    let controller = controllers[0].raw();
    assert_eq!(controller.odata_id.to_string(), CONTROLLER_ID);
    assert_eq!(controller.model, Some(Some(INTEL_MODEL.into())));
    assert_eq!(controller.firmware_version, Some(Some("9CV10420".into())));
    Ok(())
}

#[tokio::test]
async fn controllers_absent_when_not_advertised() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let storage = storage(
        bmc.clone(),
        json!({
            ODATA_ID: STORAGE_ID,
            ODATA_TYPE: "#Storage.v1_15_0.Storage",
            "Id": "1",
            "Name": "NVMe Storage"
        }),
    )
    .await?;

    assert!(storage.storage_controllers().await?.is_none());
    assert!(storage.controllers().await?.is_none());
    Ok(())
}
