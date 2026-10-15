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
//! without the quirk gets the document as it came. The caller's rules,
//! replaceable at runtime, reach the high-level wrappers as well as direct
//! reads, and every root built with them. What a write answers with is
//! the transport's reading of the reply.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::schema::manager_account::ManagerAccount;
use nv_redfish::schema::software_inventory::SoftwareInventory;
use nv_redfish::schema::software_inventory_collection::SoftwareInventoryCollection;
use nv_redfish::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::ODataId;
use nv_redfish_quirks::CompatBmc;
use nv_redfish_quirks::CompatError;
use nv_redfish_quirks::IdPattern;
use nv_redfish_quirks::Match;
use nv_redfish_quirks::UserRule;
use nv_redfish_quirks::UserRules;
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

/// A rule that drops the `ReleaseDate` HPE does not otherwise need
/// dropped.
fn undate(matches: Match) -> UserRule {
    UserRule::new("undate", matches, |mut v| {
        if let Some(document) = v.as_object_mut() {
            document.remove("ReleaseDate");
        }
        v
    })
}

/// A root with an update service, so the high-level wrappers have a path
/// to the firmware inventory.
fn service_root_with_update_service(vendor: &str) -> Value {
    let mut root = service_root(vendor);
    root["UpdateService"] = json!({ ODATA_ID: format!("{ROOT}/UpdateService") });
    root["ProtocolFeaturesSupported"] = json!({ "ExpandQuery": { "NoLinks": true } });
    root
}

fn expect_update_service(bmc: &MockBmc) {
    bmc.expect(Expect::get(
        format!("{ROOT}/UpdateService"),
        json!({
            ODATA_ID: format!("{ROOT}/UpdateService"),
            ODATA_TYPE: "#UpdateService.v1_9_0.UpdateService",
            "Id": "UpdateService",
            "Name": "Update Service",
            "FirmwareInventory": { ODATA_ID: INVENTORY },
        }),
    ));
}

fn expect_inventory(bmc: &MockBmc) {
    bmc.expect(Expect::expand(
        INVENTORY,
        json!({
            ODATA_ID: INVENTORY,
            ODATA_TYPE: "#SoftwareInventoryCollection.SoftwareInventoryCollection",
            "Name": "Firmware Inventory",
            "Members": [undated_firmware()],
        }),
    ));
}

#[test]
async fn user_rules_reach_the_wrappers_and_can_be_replaced() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(MockBmc::default());
    bmc.expect(Expect::get(ROOT, service_root_with_update_service("HPE")));
    let root = ServiceRoot::new(bmc.clone()).await?;
    expect_update_service(&bmc);
    let update_service = root.update_service().await?.expect("root links it");

    // HPE has no release-date repair: the wrapper fails as the device
    // answered.
    expect_inventory(&bmc);
    assert!(matches!(
        update_service.firmware_inventories().await,
        Err(nv_redfish::Error::Decode(_))
    ));

    // A rule installed through the root's layer applies to the wrapper
    // created before it.
    root.compat()
        .user_rules()
        .replace(vec![undate(Match::Type("SoftwareInventory".into()))]);
    expect_inventory(&bmc);
    let inventories = update_service
        .firmware_inventories()
        .await?
        .expect("service links it");
    assert!(inventories[0].raw().release_date.is_none());

    // Cleared, the next read is the device's again.
    root.compat().user_rules().clear();
    expect_inventory(&bmc);
    assert!(matches!(
        update_service.firmware_inventories().await,
        Err(nv_redfish::Error::Decode(_))
    ));
    Ok(())
}

#[test]
async fn a_user_rule_can_match_by_odata_id() -> Result<(), Box<dyn StdError>> {
    let (bmc, compat) = classified("HPE").await?;
    compat
        .user_rules()
        .replace(vec![undate(Match::Id(IdPattern::new(
            "/redfish/v1/UpdateService/FirmwareInventory/*",
        )))]);

    bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let firmware = compat.get::<SoftwareInventory>(&firmware_id()).await?;
    assert!(firmware.release_date.is_none());
    Ok(())
}

#[test]
async fn rules_given_to_service_roots_reach_each_of_them() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(MockBmc::default());
    let rules = UserRules::default();
    rules.replace(vec![undate(Match::Type("SoftwareInventory".into()))]);

    // Two roots built with the same handle, as a service that builds one
    // per iteration does.
    for _ in 0..2 {
        bmc.expect(Expect::get(ROOT, service_root("HPE")));
        let root = ServiceRoot::with_user_rules(bmc.clone(), rules.clone()).await?;
        bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
        let firmware = root
            .compat()
            .get::<SoftwareInventory>(&firmware_id())
            .await?;
        assert!(firmware.release_date.is_none());
    }

    // The caller's handle still governs every root built with it.
    bmc.expect(Expect::get(ROOT, service_root("HPE")));
    let root = ServiceRoot::with_user_rules(bmc.clone(), rules.clone()).await?;
    rules.clear();
    bmc.expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    assert!(matches!(
        root.compat().get::<SoftwareInventory>(&firmware_id()).await,
        Err(CompatError::Decode(_))
    ));
    Ok(())
}

#[test]
async fn a_write_reply_is_the_transports_reading() -> Result<(), Box<dyn StdError>> {
    let (bmc, compat) = classified("HPE").await?;
    let id = "/redfish/v1/AccountService/Accounts/1";
    let update = json!({ "Enabled": false });

    // HPE accounts omit the `AccountTypes` the schema requires. A read
    // through the layer supplies it, but the reply to an update is decoded
    // as the transport reads it, so it does not decode as an account.
    bmc.expect(Expect::update(
        id,
        &update,
        json!({
            ODATA_ID: id,
            ODATA_TYPE: "#ManagerAccount.v1_12_0.ManagerAccount",
            "Id": "1",
            "Name": "User Account",
            "UserName": "admin",
            "RoleId": "Administrator",
            "Enabled": false,
        }),
    ));
    let error = compat
        .update::<_, ManagerAccount>(&ODataId::from(id.to_owned()), None, &update)
        .await
        .expect_err("the reply is not repaired");
    assert!(matches!(error, CompatError::Decode(_)));
    Ok(())
}
