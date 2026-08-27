// SPDX-FileCopyrightText: Copyright (c) 2025 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! Integration tests of Account Service.

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

use nv_redfish::account::AccountCollection;
use nv_redfish::account::AccountService;
use nv_redfish::account::AccountTypes;
use nv_redfish::account::ManagerAccountCreate;
use nv_redfish::account::ManagerAccountUpdate;
use nv_redfish::ServiceRoot;
use nv_redfish_core::AsyncTask;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;

use serde_json::json;
use serde_json::Value as JsonValue;
use tokio::test;

const ACCOUNT_SERVICE_DATA_TYPE: &str = "#AccountService.v1_5_0.AccountService";
const ACCOUNTS_DATA_TYPE: &str = "#ManagerAccountCollection.ManagerAccountCollection";
const MANAGER_ACCOUNT_DATA_TYPE: &str = "#ManagerAccount.v1_3_0.ManagerAccount";

type TestResult<T> = Result<T, Box<dyn StdError>>;

#[test]
async fn account_request_debug_redacts_passwords() {
    const SECRET: &str = "debug-secret-sentinel";

    let create =
        ManagerAccountCreate::builder(SECRET.into(), "debug-user".into(), "Operator".into())
            .build();
    let create_debug = format!("{create:?}");
    assert!(!create_debug.contains(SECRET));
    assert!(create_debug.contains("password: \"<redacted>\""));
    assert!(create_debug.contains("user_name: \"debug-user\""));
    assert_eq!(
        serde_json::to_value(&create).expect("create request must serialize")["Password"],
        SECRET
    );

    let update = ManagerAccountUpdate::builder()
        .with_password(SECRET.into())
        .with_user_name("debug-user".into())
        .build();
    let update_debug = format!("{update:?}");
    assert!(!update_debug.contains(SECRET));
    assert!(update_debug.contains("base: None"));
    assert!(update_debug.contains("password: Some(\"<redacted>\")"));
    assert!(update_debug.contains("user_name: Some(\"debug-user\")"));
    assert_eq!(
        serde_json::to_value(&update).expect("update request must serialize")["Password"],
        SECRET
    );

    // An unset optional write-only field remains None, while a supplied value is redacted above.
    let unset_update_debug = format!("{:?}", ManagerAccountUpdate::builder().build());
    assert!(unset_update_debug.contains("password: None"));
}

#[test]
async fn additional_properties_debug_is_fully_redacted() {
    const SECRET: &str = "oem-debug-secret-sentinel";

    let update = nv_redfish::schema::resource::OemUpdate {
        additional_properties: json!({ "Secret": SECRET }),
    };
    let debug = format!("{update:?}");
    assert!(!debug.contains(SECRET));
    assert!(debug.contains("additional_properties: \"<redacted>\""));
    assert_eq!(
        serde_json::to_value(&update).expect("OEM update must serialize")["Secret"],
        SECRET
    );
}

#[test]
async fn list_accounts() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let account_service = get_account_service(bmc.clone(), &root_id, "Contoso").await?;
    let maccount_id = format!("{}/Accounts/1", account_service.raw().odata_id());
    let accounts = get_account_collection(
        bmc.clone(),
        &account_service,
        json! {[{
            ODATA_ID: maccount_id,
            ODATA_TYPE: MANAGER_ACCOUNT_DATA_TYPE,
            "Id": "1",
            "Name": "User Account",
            "UserName": "Administrator",
            "RoleId": "AdministratorRole",
            "AccountTypes": []
        }]},
    )
    .await?;
    let accounts = accounts.all_accounts_data().await?;
    assert_eq!(accounts.len(), 1);
    let account = accounts.first().unwrap().raw();
    assert_eq!(account.user_name, Some("Administrator".into()));
    assert_eq!(account.role_id, Some("AdministratorRole".into()));
    assert_eq!(account.base.name, "User Account");
    assert_eq!(account.base.id, "1");
    Ok(())
}

#[test]
async fn list_hpe_accounts() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let account_service = get_account_service(bmc.clone(), &root_id, "HPE").await?;
    let maccount_id = format!("{}/Accounts/1", account_service.raw().odata_id());
    let accounts = get_account_collection(
        bmc.clone(),
        &account_service,
        json! {[{
            ODATA_ID: maccount_id,
            ODATA_TYPE: MANAGER_ACCOUNT_DATA_TYPE,
            "Id": "1",
            "Name": "User Account",
            "UserName": "Administrator",
            "RoleId": "AdministratorRole",
        }]},
    )
    .await?;
    let accounts = accounts.all_accounts_data().await?;
    assert_eq!(accounts.len(), 1);
    let account = accounts.first().unwrap().raw();
    assert_eq!(account.user_name, Some("Administrator".into()));
    assert_eq!(account.account_types, Some(vec![AccountTypes::Redfish]));
    Ok(())
}

#[test]
async fn list_no_patch_accounts() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let account_service = get_account_service(bmc.clone(), &root_id, "Contoso").await?;
    let maccount_id = format!("{}/Accounts/1", account_service.raw().odata_id());
    assert!(get_account_collection(
        bmc.clone(),
        &account_service,
        json! {[{
            ODATA_ID: maccount_id,
            ODATA_TYPE: MANAGER_ACCOUNT_DATA_TYPE,
            "Id": "1",
            "Name": "User Account",
            "UserName": "Administrator",
            "RoleId": "AdministratorRole",
        }]},
    )
    .await
    .is_err());
    Ok(())
}

async fn get_account_service(
    bmc: Arc<Bmc>,
    root_id: &ODataId,
    vendor: &str,
) -> Result<AccountService<Bmc>, Box<dyn StdError>> {
    let account_service_id = format!("{root_id}/AccountService");
    let data_type = "#ServiceRoot.v1_13_0.ServiceRoot";

    bmc.expect(Expect::get(
        root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: &data_type,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "AccountService": {
                ODATA_ID: &account_service_id,
            },
            "Vendor": vendor,
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{root_id}/SessionService/Sessions"),
                }
            },
        }),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    let accounts_id = format!("{account_service_id}/Accounts");
    bmc.expect(Expect::get(
        &account_service_id,
        json!({
            ODATA_ID: &account_service_id,
            ODATA_TYPE: &ACCOUNT_SERVICE_DATA_TYPE,
            "Id": "AccountService",
            "Name": "AccountService",
            "Accounts": {
                ODATA_ID: &accounts_id,
            },
        }),
    ));
    Ok(service_root.account_service().await?.unwrap())
}

async fn get_account_collection(
    bmc: Arc<Bmc>,
    account_service: &AccountService<Bmc>,
    members: JsonValue,
) -> Result<AccountCollection<Bmc>, Box<dyn StdError>> {
    let accounts_id = format!("{}/Accounts", account_service.raw().odata_id());
    bmc.expect(Expect::expand(
        &accounts_id,
        json!({
            ODATA_ID: &accounts_id,
            ODATA_TYPE: &ACCOUNTS_DATA_TYPE,
            "Name": "User Accounts",
            "Members": members,
        }),
    ));
    Ok(account_service.accounts().await.map(Option::unwrap)?)
}

fn slot_member(
    accounts_id: &str,
    id: u32,
    enabled: bool,
    user_name: &str,
    etag: Option<&str>,
) -> JsonValue {
    let mut member = json!({
        ODATA_ID: format!("{accounts_id}/{id}"),
        ODATA_TYPE: MANAGER_ACCOUNT_DATA_TYPE,
        "Id": id.to_string(),
        "Name": "User Account",
        "Enabled": enabled,
        "AccountTypes": [],
        "UserName": user_name,
    });
    if let Some(etag) = etag {
        member["@odata.etag"] = JsonValue::String(etag.to_string());
    }
    member
}

async fn account_fixture(
    vendor: &str,
    slots: &[(u32, bool, &str)],
) -> TestResult<(Arc<Bmc>, String, AccountCollection<Bmc>)> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let account_service = get_account_service(bmc.clone(), &root_id, vendor).await?;
    let accounts_id = format!("{}/Accounts", account_service.raw().odata_id());

    let members = JsonValue::Array(
        slots
            .iter()
            .map(|&(id, enabled, user_name)| {
                slot_member(&accounts_id, id, enabled, user_name, None)
            })
            .collect(),
    );

    let accounts = get_account_collection(bmc.clone(), &account_service, members).await?;

    Ok((bmc, accounts_id, accounts))
}

fn async_task(location: &str, retry_after_secs: u64) -> AsyncTask {
    AsyncTask {
        location: ODataId::from(location.to_string()).into(),
        retry_after: Some(Duration::from_secs(retry_after_secs)),
    }
}

fn create_request(user_name: &str) -> ManagerAccountCreate {
    ManagerAccountCreate::builder("password".into(), user_name.to_string(), "Operator".into())
        .build()
}

fn slot_update() -> ManagerAccountUpdate {
    ManagerAccountUpdate::builder()
        .with_user_name("user".into())
        .with_password("password".into())
        .with_role_id("Operator".into())
        .with_enabled(true)
        .build()
}

fn assert_task<T>(response: ModificationResponse<T>, location: &str, retry_after_secs: u64) {
    let ModificationResponse::Task(task) = response else {
        panic!("expected an asynchronous task response");
    };

    assert_eq!(task.location.0.to_string(), location);

    assert_eq!(
        task.retry_after,
        Some(Duration::from_secs(retry_after_secs))
    );
}

fn assert_empty<T>(response: ModificationResponse<T>) {
    assert!(matches!(response, ModificationResponse::Empty));
}

fn into_entity<T>(response: ModificationResponse<T>) -> T {
    let ModificationResponse::Entity(entity) = response else {
        panic!("expected an entity response");
    };

    entity
}

// Create account (standard vendor): request includes required fields, response
// provides `AccountTypes: []` without patching.
#[test]
async fn create_account_standard_preserves_all_response_variants() -> TestResult<()> {
    let (bmc, accounts_id, accounts) = account_fixture("Contoso", &[]).await?;
    let create_req = create_request("user");
    let create_json = serde_json::to_value(&create_req).unwrap();

    bmc.expect(Expect::create(
        &accounts_id,
        create_json,
        json!({
            ODATA_ID: format!("{accounts_id}/1"),
            ODATA_TYPE: MANAGER_ACCOUNT_DATA_TYPE,
            "Id": "1",
            "Name": "User Account",
            "UserName": "user",
            "RoleId": "Operator",
            "AccountTypes": []
        }),
    ));

    let account = into_entity(accounts.create_account(create_req).await?).raw();

    assert_eq!(account.user_name, Some("user".into()));
    assert_eq!(account.role_id, Some("Operator".into()));
    assert_eq!(account.base.id, "1");
    assert_eq!(account.base.name, "User Account");
    assert!(account.account_types.as_ref().is_some_and(Vec::is_empty));

    let task_id = "/redfish/v1/TaskService/Tasks/42";
    let create_req = create_request("task-user");
    let create_json = serde_json::to_value(&create_req).unwrap();

    bmc.expect(Expect::create_task(
        &accounts_id,
        create_json,
        async_task(task_id, 7),
    ));

    assert_task(accounts.create_account(create_req).await?, task_id, 7);

    let create_req = create_request("empty-user");
    let create_json = serde_json::to_value(&create_req).unwrap();

    bmc.expect(Expect::create_empty(&accounts_id, create_json));

    assert_empty(accounts.create_account(create_req).await?);

    Ok(())
}

// Create account (HPE-like vendor): response omits `AccountTypes`, expect
// defaulting to `[Redfish]` via read patching.
#[test]
async fn create_account_hpe_patched() -> TestResult<()> {
    let (bmc, accounts_id, accounts) = account_fixture("HPE", &[]).await?;
    let create_req = create_request("user");
    let create_json = serde_json::to_value(&create_req).unwrap();

    bmc.expect(Expect::create(
        &accounts_id,
        create_json,
        json!({
            ODATA_ID: format!("{accounts_id}/1"),
            ODATA_TYPE: MANAGER_ACCOUNT_DATA_TYPE,
            "Id": "1",
            "Name": "User Account",
            "UserName": "user",
            "RoleId": "Operator"
        }),
    ));

    let account = into_entity(accounts.create_account(create_req).await?).raw();

    assert_eq!(account.user_name, Some("user".into()));
    assert_eq!(account.account_types, Some(vec![AccountTypes::Redfish]));

    Ok(())
}

// Create account (Dell slot-defined): choose first disabled slot with id >= min_slot (3).
#[test]
async fn create_account_dell_slot_defined_first_available() -> TestResult<()> {
    let (bmc, accounts_id, accounts) = account_fixture(
        "Dell",
        &[
            (1, true, "root"),
            (2, false, ""),
            (3, false, ""),
            (4, false, ""),
        ],
    )
    .await?;

    let update_req = slot_update();
    let update_json = serde_json::to_value(&update_req).unwrap();
    let account_id = format!("{accounts_id}/3");
    let etag = "slot-3-v1";

    bmc.expect(Expect::get(
        &account_id,
        slot_member(&accounts_id, 3, false, "", Some(etag)),
    ));

    bmc.expect(Expect::update(
        &account_id,
        update_json,
        json_merge([
            &slot_member(&accounts_id, 3, true, "user", None),
            &json! {{"RoleId": "Operator"}},
        ]),
    ));

    let account = into_entity(accounts.create_account(create_request("user")).await?).raw();

    assert_eq!(account.base.id, "3");
    assert_eq!(account.user_name, Some("user".into()));
    assert_eq!(account.role_id, Some("Operator".into()));
    assert_eq!(account.enabled, Some(true));

    Ok(())
}

#[test]
async fn create_account_slot_defined_rechecks_stale_candidate() -> TestResult<()> {
    let (bmc, accounts_id, accounts) =
        account_fixture("Dell", &[(3, false, ""), (4, false, "")]).await?;
    let update_req = slot_update();
    let update_json = serde_json::to_value(&update_req).unwrap();
    let stale_account_id = format!("{accounts_id}/3");
    let available_account_id = format!("{accounts_id}/4");
    let etag = "slot-4-v1";

    bmc.expect(Expect::get(
        &stale_account_id,
        slot_member(&accounts_id, 3, true, "another-user", Some("slot-3-v2")),
    ));
    bmc.expect(Expect::get(
        &available_account_id,
        slot_member(&accounts_id, 4, false, "", Some(etag)),
    ));
    bmc.expect(Expect::update(
        &available_account_id,
        update_json,
        json_merge([
            &slot_member(&accounts_id, 4, true, "user", None),
            &json! {{ "RoleId": "Operator" }},
        ]),
    ));

    let account = into_entity(accounts.create_account(create_request("user")).await?).raw();

    assert_eq!(account.base.id, "4");
    assert_eq!(account.user_name.as_deref(), Some("user"));

    Ok(())
}

#[test]
async fn create_account_slot_defined_requires_etag() -> TestResult<()> {
    let (bmc, accounts_id, accounts) = account_fixture("Dell", &[(3, false, "")]).await?;
    let account_id = format!("{accounts_id}/3");

    bmc.expect(Expect::get(
        &account_id,
        slot_member(&accounts_id, 3, false, "", None),
    ));

    assert!(matches!(
        accounts.create_account(create_request("user")).await,
        Err(nv_redfish::Error::AccountSlotNotAvailable)
    ));

    Ok(())
}

#[test]
async fn create_account_slot_defined_preserves_async_task() -> TestResult<()> {
    let (bmc, accounts_id, accounts) =
        account_fixture("Dell", &[(1, true, "root"), (3, false, "")]).await?;

    let update_req = slot_update();
    let update_json = serde_json::to_value(&update_req).unwrap();
    let account_id = format!("{accounts_id}/3");
    let task_id = "/redfish/v1/TaskService/Tasks/43";

    bmc.expect(Expect::get(
        &account_id,
        slot_member(&accounts_id, 3, false, "", Some("slot-3-v1")),
    ));

    bmc.expect(Expect::update_task(
        &account_id,
        update_json,
        async_task(task_id, 8),
    ));

    assert_task(
        accounts.create_account(create_request("user")).await?,
        task_id,
        8,
    );

    Ok(())
}

#[test]
async fn update_account_preserves_task_and_empty_responses() -> TestResult<()> {
    let (bmc, accounts_id, accounts) = account_fixture("Contoso", &[(1, true, "user")]).await?;
    let account = accounts
        .all_accounts_data()
        .await?
        .into_iter()
        .next()
        .ok_or("missing account")?;

    let account_id = format!("{accounts_id}/1");

    let update_req = ManagerAccountUpdate::builder()
        .with_password("new-password".into())
        .build();

    let update_json = serde_json::to_value(&update_req).unwrap();

    bmc.expect(Expect::update_empty(&account_id, update_json));

    assert_empty(account.update(&update_req).await?);

    let update_req = ManagerAccountUpdate::builder()
        .with_password("newer-password".into())
        .build();

    let update_json = serde_json::to_value(&update_req).unwrap();
    let task_id = "/redfish/v1/TaskService/Tasks/44";

    bmc.expect(Expect::update_task(
        &account_id,
        update_json,
        async_task(task_id, 9),
    ));

    assert_task(
        account.update_password("newer-password".into()).await?,
        task_id,
        9,
    );

    Ok(())
}

#[test]
async fn delete_account_preserves_task_and_empty_responses() -> TestResult<()> {
    let (bmc, _, accounts) =
        account_fixture("Contoso", &[(1, true, "first"), (2, true, "second")]).await?;

    let mut account_data = accounts.all_accounts_data().await?.into_iter();
    let task_account = account_data.next().ok_or("missing first account")?;
    let empty_account = account_data.next().ok_or("missing second account")?;
    let task_account_id = task_account.raw().odata_id().to_string();
    let empty_account_id = empty_account.raw().odata_id().to_string();
    let task_id = "/redfish/v1/TaskService/Tasks/45";

    bmc.expect(Expect::delete_task(
        task_account_id,
        async_task(task_id, 10),
    ));

    assert_task(task_account.delete().await?, task_id, 10);

    bmc.expect(Expect::delete(empty_account_id));

    assert_empty(empty_account.delete().await?);

    Ok(())
}

// Create account (Dell slot-defined): error when no disabled slot id >= min_slot is available.
#[test]
async fn create_account_dell_slot_defined_no_slot_available() -> TestResult<()> {
    let (_, _, accounts) = account_fixture(
        "Dell",
        &[
            (1, false, ""),
            (2, false, ""),
            (3, true, "root"),
            (4, true, "other"),
        ],
    )
    .await?;

    assert!(accounts
        .create_account(create_request("user"))
        .await
        .is_err());

    Ok(())
}

// List accounts (Dell slot-defined): disabled accounts are hidden.
#[test]
async fn list_dell_accounts_hide_disabled() -> TestResult<()> {
    let (_, _, accounts) = account_fixture(
        "Dell",
        &[(1, true, "root"), (3, false, ""), (4, true, "other")],
    )
    .await?;

    let data = accounts.all_accounts_data().await?;
    let ids: Vec<_> = data
        .into_iter()
        .map(|a| a.raw().as_ref().base.id.clone())
        .collect();

    assert_eq!(ids, vec!["1", "4"]);

    Ok(())
}
