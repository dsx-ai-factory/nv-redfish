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
//! without the quirk gets the document as it came. The caller's rules, on a
//! `RulesBmc` transport, reach the high-level wrappers, direct reads and
//! event stream payloads, through every root built on it; a document a
//! rule breaks is a decode error, and a payload a rule breaks one error in
//! its stream. What a write answers with is the transport's reading of the
//! reply.

use std::error::Error as StdError;
use std::sync::Arc;

use futures_util::TryStreamExt as _;

use nv_redfish::schema::manager_account::ManagerAccount;
use nv_redfish::schema::software_inventory::SoftwareInventory;
use nv_redfish::schema::software_inventory_collection::SoftwareInventoryCollection;
use nv_redfish::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::BmcError as _;
use nv_redfish_core::ODataId;
use nv_redfish_quirks::CompatBmc;
use nv_redfish_quirks::CompatError;
use nv_redfish_quirks::IdPattern;
use nv_redfish_quirks::Match;
use nv_redfish_quirks::RulesBmc;
use nv_redfish_quirks::UserRule;
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
const SSE: &str = "/redfish/v1/EventService/SSE";

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
async fn user_rules_reach_the_wrappers() -> Result<(), Box<dyn StdError>> {
    // HPE has no release-date repair: the wrapper fails as the device
    // answered.
    let bmc = Arc::new(MockBmc::default());
    bmc.expect(Expect::get(ROOT, service_root_with_update_service("HPE")));
    let root = ServiceRoot::new(bmc.clone()).await?;
    expect_update_service(&bmc);
    let update_service = root.update_service().await?.expect("root links it");
    expect_inventory(&bmc);
    assert!(matches!(
        update_service.firmware_inventories().await,
        Err(nv_redfish::Error::Decode(_))
    ));

    // The same device behind a transport with the caller's rule.
    let bmc = Arc::new(RulesBmc::new(
        MockBmc::default(),
        vec![undate(Match::Type("SoftwareInventory".into()))],
    ));
    let device = bmc.inner();
    device.expect(Expect::get(ROOT, service_root_with_update_service("HPE")));
    let root = ServiceRoot::new(bmc.clone()).await?;
    expect_update_service(device);
    let update_service = root.update_service().await?.expect("root links it");
    expect_inventory(device);
    let inventories = update_service
        .firmware_inventories()
        .await?
        .expect("service links it");
    assert!(inventories[0].raw().release_date.is_none());
    Ok(())
}

#[test]
async fn a_user_rule_can_match_by_odata_id() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(RulesBmc::new(
        MockBmc::default(),
        vec![undate(Match::Id(IdPattern::new(
            "/redfish/v1/UpdateService/FirmwareInventory/*",
        )))],
    ));
    bmc.inner().expect(Expect::get(ROOT, service_root("HPE")));
    let compat = CompatBmc::classify(bmc.clone()).await?;

    bmc.inner()
        .expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let firmware = compat.get::<SoftwareInventory>(&firmware_id()).await?;
    assert!(firmware.release_date.is_none());
    Ok(())
}

#[test]
async fn a_transports_rules_reach_every_root_built_on_it() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(RulesBmc::new(
        MockBmc::default(),
        vec![undate(Match::Type("SoftwareInventory".into()))],
    ));

    // Two roots over the one transport, as a service that builds one per
    // iteration does.
    for _ in 0..2 {
        bmc.inner().expect(Expect::get(ROOT, service_root("HPE")));
        let root = ServiceRoot::new(bmc.clone()).await?;
        bmc.inner()
            .expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
        let firmware = root
            .compat()
            .get::<SoftwareInventory>(&firmware_id())
            .await?;
        assert!(firmware.release_date.is_none());
    }
    Ok(())
}

#[test]
async fn a_read_around_the_layer_gets_the_callers_rules() -> Result<(), Box<dyn StdError>> {
    let bmc = RulesBmc::new(
        MockBmc::default(),
        vec![undate(Match::Type("SoftwareInventory".into()))],
    );
    bmc.inner()
        .expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let firmware = bmc.get::<SoftwareInventory>(&firmware_id()).await?;
    assert!(firmware.release_date.is_none());
    Ok(())
}

#[test]
async fn the_callers_rules_reach_event_stream_payloads() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(RulesBmc::new(
        MockBmc::default(),
        vec![UserRule::new(
            "tag",
            Match::Type("Event".into()),
            |mut v| {
                v["Context"] = json!("tagged");
                v
            },
        )],
    ));
    bmc.inner().expect(Expect::get(ROOT, service_root("HPE")));
    let compat = CompatBmc::classify(bmc.clone()).await?;

    bmc.inner().expect(Expect::stream_events(
        SSE,
        None,
        [(
            Some("1"),
            json!({
                ODATA_TYPE: "#Event.v1_9_0.Event",
                "Id": "1",
                "Name": "Event Log",
                "Events": [],
            }),
        )],
    ));
    let mut stream = compat.stream_events::<Value>(SSE, None).await?;
    let event = stream.try_next().await?.ok_or("event")?;
    assert_eq!(event.data["Context"], "tagged");
    assert_eq!(event.last_event_id.as_deref(), Some("1"));
    Ok(())
}

#[test]
async fn a_document_a_rule_breaks_is_a_decode_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(RulesBmc::new(
        MockBmc::default(),
        vec![UserRule::new(
            "break",
            Match::Type("SoftwareInventory".into()),
            |mut v| {
                v["Version"] = json!(7);
                v
            },
        )],
    ));
    bmc.inner().expect(Expect::get(ROOT, service_root("Dell")));
    let compat = CompatBmc::classify(bmc.clone()).await?;

    bmc.inner()
        .expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let through_layer = compat.get::<SoftwareInventory>(&firmware_id()).await;
    assert!(matches!(through_layer, Err(CompatError::Decode(_))));

    // Read around the layer, the transport reports it as its own.
    bmc.inner()
        .expect(Expect::get(BMC_FIRMWARE, undated_firmware()));
    let Err(around) = bmc.get::<SoftwareInventory>(&firmware_id()).await else {
        return Err("the rule breaks the document".into());
    };
    assert!(around.into_decode_error().is_ok());
    Ok(())
}

/// An event payload, read only for its `Id`.
#[derive(Debug, serde::Deserialize)]
struct EventId {
    #[serde(rename = "Id")]
    id: String,
}

#[test]
async fn a_payload_a_rule_breaks_is_one_error_in_its_stream() -> Result<(), Box<dyn StdError>> {
    let bmc = RulesBmc::new(
        MockBmc::default(),
        vec![UserRule::new(
            "break the first",
            Match::Type("Event".into()),
            |mut v| {
                if v["Id"] == "1" {
                    v["Id"] = json!(1);
                }
                v
            },
        )],
    );
    let event = |id: &str| json!({ ODATA_TYPE: "#Event.v1_9_0.Event", "Id": id });
    bmc.inner().expect(Expect::stream_events(
        SSE,
        None,
        [(Some("1"), event("1")), (Some("2"), event("2"))],
    ));
    let mut stream = bmc.stream_events::<EventId>(SSE, None).await?;

    let Err(broken) = stream.try_next().await else {
        return Err("the rule breaks the first payload".into());
    };
    assert!(broken.into_decode_error().is_ok());
    let next = stream.try_next().await?.ok_or("the stream goes on")?;
    assert_eq!(next.data.id, "2");
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
