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

//! The compatibility layer: a raw schema read through `CompatBmc` carries
//! the platform's document repairs — the Dell firmware release date here —
//! at the top level and inside an expanded collection, and a platform
//! without the quirk gets the document as it came.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::schema::software_inventory::SoftwareInventory;
use nv_redfish::schema::software_inventory_collection::SoftwareInventoryCollection;
use nv_redfish::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::ODataId;
use nv_redfish_quirks::CompatBmc;
use nv_redfish_quirks::CompatError;
use nv_redfish_tests::Bmc as MockBmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;

const ROOT: &str = "/redfish/v1";
const INVENTORY: &str = "/redfish/v1/UpdateService/FirmwareInventory";
const BMC_FIRMWARE: &str = "/redfish/v1/UpdateService/FirmwareInventory/BMC";

fn service_root(vendor: &str) -> Value {
    json!({
        ODATA_ID: ROOT,
        ODATA_TYPE: "#ServiceRoot.v1_15_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root Service",
        "Vendor": vendor,
        "Links": { "Sessions": { ODATA_ID: format!("{ROOT}/SessionService/Sessions") } },
    })
}

/// A firmware component dated the way iDRAC dates most of them.
fn undated_firmware() -> Value {
    json!({
        ODATA_ID: BMC_FIRMWARE,
        ODATA_TYPE: "#SoftwareInventory.v1_4_0.SoftwareInventory",
        "Id": "BMC",
        "Name": "Integrated Remote Access Controller",
        "Version": "7.10.30.00",
        "ReleaseDate": "00:00:00Z",
    })
}

fn firmware_id() -> ODataId {
    ODataId::from(BMC_FIRMWARE.to_owned())
}

/// A layer classified from a `vendor`'s service root, with the mock it
/// wraps so the test can queue the device's next answers.
async fn classified(vendor: &str) -> Result<(Arc<MockBmc>, CompatBmc<MockBmc>), Box<dyn StdError>> {
    let bmc = Arc::new(MockBmc::default());
    bmc.expect(Expect::get(ROOT, service_root(vendor)));
    let compat = CompatBmc::classify(bmc.clone()).await?;
    Ok((bmc, compat))
}

#[test]
async fn a_raw_read_carries_the_platforms_repairs() -> Result<(), Box<dyn StdError>> {
    let (bmc, compat) = classified("Dell").await?;

    bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let firmware = compat.get::<SoftwareInventory>(&firmware_id()).await?;
    assert!(firmware.release_date.is_none());
    assert_eq!(
        firmware.version.clone().flatten().as_deref(),
        Some("7.10.30.00")
    );

    // The same document read past the layer does not decode: the repair
    // is the layer's, not the schema type's.
    bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    assert!(bmc.get::<SoftwareInventory>(&firmware_id()).await.is_err());
    Ok(())
}

#[test]
async fn a_member_expanded_inline_is_repaired_at_depth() -> Result<(), Box<dyn StdError>> {
    let (bmc, compat) = classified("Dell").await?;

    bmc.expect(Expect::get(
        INVENTORY,
        json!({
            ODATA_ID: INVENTORY,
            ODATA_TYPE: "#SoftwareInventoryCollection.SoftwareInventoryCollection",
            "Name": "Firmware Inventory",
            "Members@odata.count": 1,
            "Members": [undated_firmware()],
        }),
    ));
    let collection = compat
        .get::<SoftwareInventoryCollection>(&ODataId::from(INVENTORY.to_owned()))
        .await?;
    // Nothing else is queued: an expanded member resolves without a
    // request, already repaired.
    let member = collection.members[0].get(&compat).await?;
    assert!(member.release_date.is_none());
    Ok(())
}

#[test]
async fn another_platforms_documents_come_back_as_they_came() -> Result<(), Box<dyn StdError>> {
    let (bmc, compat) = classified("HPE").await?;

    bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let error = compat
        .get::<SoftwareInventory>(&firmware_id())
        .await
        .expect_err("HPE has no release-date repair, so the document fails as it would raw");
    assert!(matches!(error, CompatError::Decode(_)));
    Ok(())
}

#[test]
async fn a_transport_failure_comes_back_as_the_transports() -> Result<(), Box<dyn StdError>> {
    let (_bmc, compat) = classified("Dell").await?;

    // Nothing queued: the mock refuses the GET, and the layer reports the
    // transport's failure rather than a decode of its own.
    let error = compat
        .get::<SoftwareInventory>(&firmware_id())
        .await
        .expect_err("the mock has no answer queued");
    assert!(matches!(error, CompatError::Transport(_)));
    Ok(())
}

#[test]
async fn a_service_root_hands_out_its_layer() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(MockBmc::default());
    bmc.expect(Expect::get(ROOT, service_root("Dell")));
    let root = ServiceRoot::new(bmc.clone()).await?;
    let compat = root.compat();

    bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let firmware = compat.get::<SoftwareInventory>(&firmware_id()).await?;
    assert!(firmware.release_date.is_none());
    Ok(())
}
