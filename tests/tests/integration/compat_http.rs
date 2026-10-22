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

//! The compatibility layer over the HTTP transport. Repairs run in the
//! transport's own decode; layers over one transport share its cached
//! documents, the caller's rules on the transport included, and a layer
//! that repairs nothing shares the entries of plain reads; a document that
//! does not match its type is a decode error; and a write keeps the
//! transport's reading of the reply, with the platform's protocol quirks
//! applied.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::account::AccountServiceConfig;
use nv_redfish::account::ManagerAccountUpdate;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::schema::software_inventory::SoftwareInventory;
use nv_redfish::schema::software_inventory_collection::SoftwareInventoryCollection;
use nv_redfish::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use nv_redfish_core::Raw;
use nv_redfish_quirks::BmcQuirks;
use nv_redfish_quirks::CompatBmc;
use nv_redfish_quirks::CompatError;
use nv_redfish_quirks::Match;
use nv_redfish_quirks::RootEvidence;
use nv_redfish_quirks::RulesBmc;
use nv_redfish_quirks::UserRule;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;
use url::Url;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;

const FIRMWARE: &str = "/redfish/v1/UpdateService/FirmwareInventory/BMC";
const INVENTORY: &str = "/redfish/v1/UpdateService/FirmwareInventory";
const ETAG: &str = "W/\"1\"";

type Layer = CompatBmc<HttpBmc<Client>>;

/// A firmware component dated the way iDRAC dates most of them.
fn undated_firmware() -> Value {
    json!({
        ODATA_ID: FIRMWARE,
        ODATA_TYPE: "#SoftwareInventory.v1_4_0.SoftwareInventory",
        "Id": "BMC",
        "Name": "Integrated Remote Access Controller",
        "Version": "7.10.30.00",
        "ReleaseDate": "00:00:00Z",
    })
}

fn id(path: &str) -> ODataId {
    ODataId::from(path.to_owned())
}

fn http(server: &MockServer) -> Result<HttpBmc<Client>, Box<dyn StdError>> {
    Ok(HttpBmc::new(
        Client::new()?,
        Url::parse(&server.uri())?,
        BmcCredentials::new("root".into(), "password".into()),
        CacheSettings::default(),
    ))
}

fn transport(server: &MockServer) -> Result<Arc<HttpBmc<Client>>, Box<dyn StdError>> {
    Ok(Arc::new(http(server)?))
}

/// The platform a `vendor`'s service root classifies into.
fn platform(vendor: &str) -> Arc<BmcQuirks> {
    Arc::new(BmcQuirks::classify(&RootEvidence::from_root(
        &json!({ "Vendor": vendor }),
    )))
}

/// Serves `body` at `at` with an `ETag`, and `304` to a request that
/// revalidates it.
async fn serve_cached(server: &MockServer, at: &str, body: &Value) {
    Mock::given(method("GET"))
        .and(path(at))
        .and(header("if-none-match", ETAG))
        .respond_with(ResponseTemplate::new(304))
        .with_priority(1)
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(at))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(body)
                .insert_header("etag", ETAG),
        )
        .mount(server)
        .await;
}

/// Whether the last request the server saw revalidated a cached document.
async fn last_was_revalidation(server: &MockServer) -> bool {
    server
        .received_requests()
        .await
        .and_then(|requests| requests.last().cloned())
        .is_some_and(|request| request.headers.contains_key("if-none-match"))
}

#[test]
async fn layers_with_the_same_repairs_share_cached_documents() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    serve_cached(&server, FIRMWARE, &undated_firmware()).await;
    let http = transport(&server)?;
    // What two `ServiceRoot::new` calls over one transport build.
    let first: Layer = CompatBmc::new(http.clone(), platform("Dell"));
    let second: Layer = CompatBmc::new(http, platform("Dell"));

    let read = first.get::<SoftwareInventory>(&id(FIRMWARE)).await?;
    assert!(read.release_date.is_none(), "the Dell repair ran");
    let again = second.get::<SoftwareInventory>(&id(FIRMWARE)).await?;
    assert!(last_was_revalidation(&server).await);
    assert!(Arc::ptr_eq(&read, &again));
    Ok(())
}

#[test]
async fn a_layer_without_repairs_shares_plain_reads() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    serve_cached(&server, FIRMWARE, &undated_firmware()).await;
    let http = transport(&server)?;
    let layer: Layer = CompatBmc::new(http.clone(), platform("Contoso"));

    let plain = http.get::<Raw>(&id(FIRMWARE)).await?;
    let through_layer = layer.get::<Raw>(&id(FIRMWARE)).await?;
    assert!(last_was_revalidation(&server).await);
    assert!(Arc::ptr_eq(&plain, &through_layer));
    Ok(())
}

#[test]
async fn a_transports_rules_are_cached_with_the_document() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    serve_cached(&server, FIRMWARE, &undated_firmware()).await;
    let rename = UserRule::new(
        "rename",
        Match::Type("SoftwareInventory".into()),
        |mut document| {
            document["Name"] = json!("Renamed");
            document
        },
    );
    let ruled = Arc::new(RulesBmc::new(http(&server)?, vec![rename]));
    // What two `ServiceRoot::new` calls over one transport build.
    let first = CompatBmc::new(ruled.clone(), platform("Dell"));
    let second = CompatBmc::new(ruled, platform("Dell"));

    let read = first.get::<SoftwareInventory>(&id(FIRMWARE)).await?;
    assert!(read.release_date.is_none(), "the Dell repair ran");
    assert_eq!(read.name, "Renamed", "the caller's rule ran after it");
    let again = second.get::<SoftwareInventory>(&id(FIRMWARE)).await?;
    assert!(last_was_revalidation(&server).await);
    assert!(Arc::ptr_eq(&read, &again));
    Ok(())
}

#[test]
async fn a_repair_reaches_a_member_inlined_in_a_collection() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    serve_cached(
        &server,
        INVENTORY,
        &json!({
            ODATA_ID: INVENTORY,
            ODATA_TYPE: "#SoftwareInventoryCollection.SoftwareInventoryCollection",
            "Name": "Firmware Inventory",
            "Members": [undated_firmware()],
            "Members@odata.count": 1,
        }),
    )
    .await;
    let layer: Layer = CompatBmc::new(transport(&server)?, platform("Dell"));

    let inventory = layer
        .get::<SoftwareInventoryCollection>(&id(INVENTORY))
        .await?;
    assert_eq!(inventory.members.len(), 1);
    Ok(())
}

#[test]
async fn a_document_that_does_not_match_its_type_is_a_decode_error() -> Result<(), Box<dyn StdError>>
{
    let server = MockServer::start().await;
    serve_cached(&server, FIRMWARE, &undated_firmware()).await;
    Mock::given(method("PATCH"))
        .and(path(FIRMWARE))
        .respond_with(ResponseTemplate::new(200).set_body_json(undated_firmware()))
        .mount(&server)
        .await;
    // A platform without the Dell repair reads the date as it came.
    let layer: Layer = CompatBmc::new(transport(&server)?, platform("Contoso"));

    let read = layer.get::<SoftwareInventory>(&id(FIRMWARE)).await;
    assert!(
        matches!(read, Err(CompatError::Decode(_))),
        "a read: {:?}",
        read.err()
    );
    let write = layer
        .update::<_, SoftwareInventory>(&id(FIRMWARE), None, &json!({ "Version": "1" }))
        .await;
    assert!(
        matches!(write, Err(CompatError::Decode(_))),
        "a write reply: {:?}",
        write.err()
    );
    Ok(())
}

#[test]
async fn a_write_keeps_the_transports_reading_of_a_success_reply() -> Result<(), Box<dyn StdError>>
{
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(FIRMWARE))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "Success": { "code": "Base.1.10.Success", "message": "Successfully Completed Request." }
        })))
        .mount(&server)
        .await;
    let layer: Layer = CompatBmc::new(transport(&server)?, platform("Supermicro"));

    let reply = layer
        .update::<_, SoftwareInventory>(&id(FIRMWARE), None, &json!({ "Version": "1" }))
        .await?;
    let ModificationResponse::Message(body) = reply else {
        return Err(format!("expected Message, got {:?}", reply).into());
    };
    assert_eq!(
        body,
        json!({
            "Success": {
                "code": "Base.1.10.Success",
                "message": "Successfully Completed Request."
            }
        })
    );
    Ok(())
}

#[test]
async fn a_staged_change_without_a_task_is_empty_only_on_supermicro(
) -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(FIRMWARE))
        .respond_with(ResponseTemplate::new(202))
        .mount(&server)
        .await;
    let http = transport(&server)?;
    let update = json!({ "Version": "1" });

    let supermicro: Layer = CompatBmc::new(http.clone(), platform("Supermicro"));
    let reply = supermicro
        .update::<_, SoftwareInventory>(&id(FIRMWARE), None, &update)
        .await?;
    assert!(matches!(reply, ModificationResponse::Empty));

    let contoso: Layer = CompatBmc::new(http, platform("Contoso"));
    let reply = contoso
        .update::<_, SoftwareInventory>(&id(FIRMWARE), None, &update)
        .await;
    assert!(matches!(reply, Err(CompatError::Transport(_))));
    Ok(())
}

#[test]
async fn ami_firmware_is_sent_a_wildcard_if_match() -> Result<(), Box<dyn StdError>> {
    let server = MockServer::start().await;
    Mock::given(method("PATCH"))
        .and(path(FIRMWARE))
        .and(header("if-match", "*"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(FIRMWARE))
        .and(header("if-match", ETAG))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let http = transport(&server)?;
    let etag = ODataETag::from(ETAG.to_owned());
    let update = json!({ "Version": "1" });

    let ami: Layer = CompatBmc::new(http.clone(), platform("AMI"));
    let reply = ami
        .update::<_, SoftwareInventory>(&id(FIRMWARE), Some(&etag), &update)
        .await?;
    assert!(matches!(reply, ModificationResponse::Empty));
    let dell: Layer = CompatBmc::new(http, platform("Dell"));
    let reply = dell
        .update::<_, SoftwareInventory>(&id(FIRMWARE), Some(&etag), &update)
        .await?;
    assert!(matches!(reply, ModificationResponse::Empty));
    Ok(())
}

#[test]
async fn an_hpe_account_update_answered_with_a_success_message_is_empty(
) -> Result<(), Box<dyn StdError>> {
    const ROOT: &str = "/redfish/v1";
    const ACCOUNT_SERVICE: &str = "/redfish/v1/AccountService";
    const ACCOUNTS: &str = "/redfish/v1/AccountService/Accounts";
    const ACCOUNT: &str = "/redfish/v1/AccountService/Accounts/1";

    let server = MockServer::start().await;
    for (at, body) in [
        (
            ROOT,
            json!({
                ODATA_ID: ROOT,
                ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
                "Id": "RootService",
                "Name": "Root Service",
                "Vendor": "HPE",
                "AccountService": { ODATA_ID: ACCOUNT_SERVICE },
                "Links": { "Sessions": { ODATA_ID: format!("{ROOT}/SessionService/Sessions") } },
            }),
        ),
        (
            ACCOUNT_SERVICE,
            json!({
                ODATA_ID: ACCOUNT_SERVICE,
                ODATA_TYPE: "#AccountService.v1_5_0.AccountService",
                "Id": "AccountService",
                "Name": "Account Service",
                "Accounts": { ODATA_ID: ACCOUNTS },
            }),
        ),
        (
            ACCOUNTS,
            json!({
                ODATA_ID: ACCOUNTS,
                ODATA_TYPE: "#ManagerAccountCollection.ManagerAccountCollection",
                "Name": "User Accounts",
                "Members": [{ ODATA_ID: ACCOUNT }],
            }),
        ),
        // iLO's account, without the `AccountTypes` the schema requires.
        (
            ACCOUNT,
            json!({
                ODATA_ID: ACCOUNT,
                ODATA_TYPE: "#ManagerAccount.v1_3_0.ManagerAccount",
                "Id": "1",
                "Name": "User Account",
                "Enabled": true,
                "UserName": "user",
                "RoleId": "Operator",
            }),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
    }
    Mock::given(method("PATCH"))
        .and(path(ACCOUNT))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": { "code": "Base.1.8.Success", "message": "Successfully Completed Request." }
        })))
        .mount(&server)
        .await;

    let root = ServiceRoot::new(transport(&server)?).await?;
    let accounts = root
        .account_service(AccountServiceConfig::standard())
        .await?
        .ok_or("account service")?
        .accounts()
        .await?
        .ok_or("accounts")?;
    let account = accounts
        .all_accounts_data()
        .await?
        .into_iter()
        .next()
        .ok_or("account")?;
    let update = ManagerAccountUpdate::builder()
        .with_role_id("Operator".into())
        .build();

    // The reply is not an account, so the transport reads the success
    // message as it would for any account update.
    let reply = account.update(&update).await?;
    let ModificationResponse::Message(body) = reply else {
        return Err("expected ModificationResponse::Message".into());
    };
    assert_eq!(
        body,
        json!({
            "error": {
                "code": "Base.1.8.Success",
                "message": "Successfully Completed Request."
            }
        })
    );
    Ok(())
}

#[test]
async fn computer_system_reset_preserves_success_message_for_strict_inspection(
) -> Result<(), Box<dyn StdError>> {
    use nv_redfish::bmc_http::strict_success_code;
    use nv_redfish::bmc_http::success_code;
    use nv_redfish::bmc_http::SuccessCode;
    use nv_redfish::resource::ResetType;

    const ROOT: &str = "/redfish/v1";
    const SYSTEMS: &str = "/redfish/v1/Systems";
    const SYSTEM: &str = "/redfish/v1/Systems/1";
    const RESET: &str = "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset";

    let cases = [
        (
            json!({
                "error": {
                    "code": "Base.1.15.Success",
                    "message": "Successfully Completed Request",
                    "@Message.ExtendedInfo": [{ "MessageId": "Base.1.15.Success", "Severity": "OK" }]
                }
            }),
            Some(SuccessCode::Success),
            Some(SuccessCode::Success),
        ),
        (
            json!({
                "error": {
                    "code": "Base.1.15.Success",
                    "message": "Successfully Completed Request",
                    "@Message.ExtendedInfo": [
                        { "MessageId": "Base.1.15.Success", "Severity": "OK" },
                        { "MessageId": "Base.1.15.NoOperation", "Severity": "Warning" }
                    ]
                }
            }),
            Some(SuccessCode::Success),
            None,
        ),
    ];

    for (reset_reply, expected_success, expected_strict) in cases {
        let server = MockServer::start().await;
        for (at, body) in [
            (
                ROOT,
                json!({
                    ODATA_ID: ROOT,
                    ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
                    "Id": "RootService",
                    "Name": "Root Service",
                    "Systems": { ODATA_ID: SYSTEMS },
                    "Links": { "Sessions": { ODATA_ID: format!("{ROOT}/SessionService/Sessions") } },
                }),
            ),
            (
                SYSTEMS,
                json!({
                    ODATA_ID: SYSTEMS,
                    ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
                    "Name": "Computer System Collection",
                    "Members": [{ ODATA_ID: SYSTEM }],
                }),
            ),
            (
                SYSTEM,
                json!({
                    ODATA_ID: SYSTEM,
                    ODATA_TYPE: "#ComputerSystem.v1_20_0.ComputerSystem",
                    "Id": "1",
                    "Name": "System",
                    "Actions": {
                        "#ComputerSystem.Reset": {
                            "target": RESET,
                        }
                    }
                }),
            ),
        ] {
            Mock::given(method("GET"))
                .and(path(at))
                .respond_with(ResponseTemplate::new(200).set_body_json(body))
                .mount(&server)
                .await;
        }
        Mock::given(method("POST"))
            .and(path(RESET))
            .respond_with(ResponseTemplate::new(200).set_body_json(&reset_reply))
            .mount(&server)
            .await;

        let root = ServiceRoot::new(transport(&server)?).await?;
        let systems = root.systems().await?.ok_or("systems")?;
        let system = systems
            .members()
            .await?
            .into_iter()
            .next()
            .ok_or("system")?;

        let response = system.reset(Some(ResetType::ForceRestart)).await?;
        let ModificationResponse::Message(body) = response else {
            return Err(
                format!("expected ModificationResponse::Message, got {:?}", response).into(),
            );
        };
        assert_eq!(body, reset_reply);
        assert_eq!(success_code(&body), expected_success);
        assert_eq!(strict_success_code(&body), expected_strict);
    }

    Ok(())
}
