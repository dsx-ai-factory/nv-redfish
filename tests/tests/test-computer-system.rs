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
//! Integration tests for Computer System resources.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::computer_system::BootOptionReference;
use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::computer_system::SystemCollection;
use nv_redfish::resource::ResetType;
use nv_redfish::Resource;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::ami_viking_service_root;
use nv_redfish_tests::anonymous_1_9_service_root;
use nv_redfish_tests::liteon_powershelf_service_root;
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

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_20_0.ComputerSystem";

#[test]
async fn reset_invokes_computer_system_reset_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let action_target = format!("{}/Actions/ComputerSystem.Reset", ids.system_id);
    let system = get_system(
        bmc.clone(),
        &ids,
        computer_system(
            &ids,
            redfish_action_payload("ComputerSystem.Reset", &action_target),
        ),
    )
    .await?;

    expect_redfish_reset_action(&bmc, &action_target, Some("GracefulRestart"));

    assert!(matches!(
        system.reset(Some(ResetType::GracefulRestart)).await?,
        ModificationResponse::Entity(())
    ));

    expect_redfish_reset_action(&bmc, &action_target, None);

    assert!(matches!(
        system.reset(None).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn set_boot_order_preserves_task_and_empty_responses() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();

    let system = get_system(
        bmc.clone(),
        &ids,
        computer_system(&ids, json!({ "Boot": { "BootOrder": ["Boot0001"] } })),
    )
    .await?;

    let task_id = "/redfish/v1/TaskService/Tasks/52";

    bmc.expect(Expect::update_task(
        &ids.system_id,
        json!({ "Boot": { "BootOrder": ["Boot0002"] } }),
        async_task(task_id, 7),
    ));

    assert_task(
        system
            .set_boot_order(vec![BootOptionReference::new("Boot0002".into())])
            .await?,
        task_id,
        7,
    );

    bmc.expect(Expect::update_empty(
        &ids.system_id,
        json!({ "Boot": { "BootOrder": ["Boot0003"] } }),
    ));

    assert_empty(
        system
            .set_boot_order(vec![BootOptionReference::new("Boot0003".into())])
            .await?,
    );

    Ok(())
}

#[test]
async fn reset_returns_action_not_available_when_computer_system_reset_is_absent(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        computer_system(&ids, redfish_empty_actions_payload()),
    )
    .await?;

    assert!(matches!(
        system.reset(Some(ResetType::GracefulRestart)).await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));

    Ok(())
}

#[test]
async fn dell_wrong_last_reset_time_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let computer_system = computer_system(
        &ids,
        json!({ "LastResetTime": "0000-00-00T00:00:00+00:00" }),
    );
    let systems = get_systems(bmc.clone(), &ids, "Dell", vec![computer_system]).await?;

    let members = systems.members().await?;
    assert_eq!(members.len(), 1);
    let system = &members[0];
    assert!(system.raw().last_reset_time.is_none());

    Ok(())
}

#[test]
async fn ami_viking_missing_root_systems_nav_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let computer_system = computer_system(&ids, json!({}));
    let service_root = expect_viking_service_root_without_systems(bmc.clone(), &ids).await?;
    bmc.expect(Expect::get(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Computer System Collection",
            "Members": [computer_system]
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn liteon_f16_missing_root_systems_nav_workaround() -> Result<(), Box<dyn StdError>> {
    // Platform under test: Lite-On F16 power shelf (`Vendor=LITE-ON TECHNOLOGY CORP.`).
    // Quirk under test: missing root Systems navigation property; `/Systems` is fetched directly.
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let computer_system = computer_system(&ids, json!({}));
    let service_root =
        expect_liteon_powershelf_service_root_without_systems(bmc.clone(), &ids).await?;
    bmc.expect(Expect::get(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Computer System Collection",
            "Members": [computer_system]
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn anonymous_1_9_0_missing_root_systems_nav_workaround() -> Result<(), Box<dyn StdError>> {
    // Platform under test: anonymous Redfish 1.9.0 root (no vendor).
    // Quirk under test: missing root Systems navigation property.
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let computer_system = computer_system(&ids, json!({}));
    let service_root = expect_anonymous_1_9_service_root_without_systems(bmc.clone(), &ids).await?;
    bmc.expect(Expect::get(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Computer System Collection",
            "Members": [computer_system]
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn nvidia_dpu_empty_system_uuid_in_expanded_members_workaround(
) -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVIDIA DPU (`Vendor=Nvidia`, `Product=Nvidia-BMCMezz`).
    // Quirk under test: ComputerSystem.UUID="" in inline collection members.
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let service_root = expect_nvidia_dpu_service_root(bmc.clone(), &ids).await?;
    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Computer System Collection",
            "Members": [
                computer_system(&ids, json!({ "UUID": "" }))
            ]
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].raw().uuid, Some(None));

    Ok(())
}

// Check that collection with {"Members":null} returns empty collection for Bluefield BMC.
#[test]
async fn null_collection_member_test() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids_blue_field();
    let service_root = expect_nvidia_dpu_service_root_bf3(bmc.clone()).await?;

    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Computer System Collection",
            "Members": [
                computer_system(&ids, json!({
                "Storage": { "@odata.id": "/redfish/v1/Systems/Bluefield/Storage" },
                "Boot": {
                    "BootOptions": {
                        "@odata.id": "/redfish/v1/Systems/Bluefield/BootOptions"
                    },
                }
                }))
            ]
        }),
    ));

    bmc.expect(Expect::expand(
        "/redfish/v1/Systems/Bluefield/BootOptions",
        json!({
            "@odata.id": "/redfish/v1/Systems/Bluefield/BootOptions",
            "@odata.type": "#BootOptionCollection.BootOptionCollection",
            "Members": null,
            "Members@odata.count": 0,
            "Name": "Boot Option Collection"
        }),
    ));

    bmc.expect(Expect::get(
        "/redfish/v1/AccountService",
        json!(
            {
                "@odata.id": "/redfish/v1/AccountService",
                "@odata.type": "#AccountService.v1_15_0.AccountService",
                "Name": "Account Service",
                "Accounts": {
                    "@odata.id": "/redfish/v1/AccountService/Accounts"
                },
                "Id": "AccountService",
                "MultiFactorAuth": {
                    "ClientCertificate": {
                        "Certificates": {
                            "@odata.id": "/redfish/v1/AccountService/MultiFactorAuth/ClientCertificate/Certificates",
                            "@odata.type": "#CertificateCollection.CertificateCollection",
                            "Members": null,
                            "Members@odata.count": 0
                        },
                    }
                }
            }
        ),
    ));

    bmc.expect(Expect::expand(
        "/redfish/v1/Systems/Bluefield/Storage",
        json!({
        "@odata.id": "/redfish/v1/Systems/Bluefield/Storage",
        "@odata.type": "#StorageCollection.StorageCollection",
        "Members": null,
        "Members@odata.count": 0,
        "Name": "Storage Collection"
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    let systems = systems.members().await?;
    let boot_options = systems[0].boot_options().await?;
    let members = boot_options.unwrap().members().await?;

    assert_eq!(members.len(), 0);

    let _account_service = service_root.account_service().await?.unwrap().raw();

    let storage = systems[0].storage_controllers().await?.unwrap();
    assert_eq!(storage.len(), 0);

    Ok(())
}

#[test]
async fn nvidia_dpu_empty_system_uuid_on_member_fetch_workaround() -> Result<(), Box<dyn StdError>>
{
    // Platform under test: NVIDIA DPU (`Vendor=Nvidia`, `Product=Nvidia-BMCMezz`).
    // Quirk under test: ComputerSystem.UUID="" in member payload fetched by link.
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();
    let service_root = expect_nvidia_dpu_service_root(bmc.clone(), &ids).await?;
    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Computer System Collection",
            "Members": [
                {
                    ODATA_ID: &ids.system_id
                }
            ]
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    bmc.expect(Expect::get(
        &ids.system_id,
        computer_system(&ids, json!({ "UUID": "" })),
    ));
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].raw().uuid, Some(None));

    Ok(())
}

async fn get_systems(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
    vendor: &str,
    members: Vec<Value>,
) -> Result<SystemCollection<Bmc>, Box<dyn StdError>> {
    let service_root = expect_service_root(bmc.clone(), ids, vendor).await?;
    let systems_name = resource_name(&ids.systems_id);
    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: &SYSTEM_COLLECTION_DATA_TYPE,
            "Id": systems_name,
            "Name": "Computer System Collection",
            "Members": members
        }),
    ));

    service_root
        .systems()
        .await
        .map(Option::unwrap)
        .map_err(Into::into)
}

async fn expect_nvidia_dpu_service_root(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json!({
            ODATA_ID: &ids.root_id,
            ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "Systems": { ODATA_ID: &ids.systems_id },
            "Vendor": "Nvidia",
            "Product": "Nvidia-BMCMezz",
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));

    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_nvidia_dpu_service_root_bf3(
    bmc: Arc<Bmc>,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
            "AccountService": {
                "@odata.id": "/redfish/v1/AccountService"
            },
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "Systems": { ODATA_ID: &systems_id },
            "Vendor": "Nvidia",
            "Product": "BlueField-3 DPU",
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", &root_id),
                }
            },
        }),
    ));

    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_service_root(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
    vendor: &str,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json!({
            ODATA_ID: &ids.root_id,
            ODATA_TYPE: &SERVICE_ROOT_DATA_TYPE,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "Systems": { ODATA_ID: &ids.systems_id },
            "Vendor": vendor,
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));

    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_viking_service_root_without_systems(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        ami_viking_service_root(&ids.root_id, json!({})),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_anonymous_1_9_service_root_without_systems(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        anonymous_1_9_service_root(&ids.root_id, json!({})),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_liteon_powershelf_service_root_without_systems(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        liteon_powershelf_service_root(&ids.root_id, json!({})),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

#[test]
async fn viking_with_garbage_in_computer_systems() -> Result<(), Box<dyn StdError>> {
    // Viking response with the payload: HGX_Baseboard_0/LogServices/FDR should be filtered out.
    let bmc = Arc::new(Bmc::default());
    let ids = computer_system_ids();

    // Viking service root
    bmc.expect(Expect::get(
        &ids.root_id,
        ami_viking_service_root(
            &ids.root_id,
            json!({
                "Systems": { ODATA_ID: &ids.systems_id }
            }),
        ),
    ));

    let service_root = ServiceRoot::new(bmc.clone()).await?;

    // Collection with garbage entry that should be filtered out
    let dgx_id = format!("{}/DGX", ids.systems_id);
    let hgx_id = format!("{}/HGX_Baseboard_0", ids.systems_id);
    let garbage_id = format!("{}/HGX_Baseboard_0/LogServices/FDR", ids.systems_id);

    bmc.expect(Expect::get(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: SYSTEM_COLLECTION_DATA_TYPE,
            "Id": resource_name(&ids.systems_id),
            "Name": "Systems Collection",
            "Members": [
                json!({ ODATA_ID: &dgx_id }),
                json!({ ODATA_ID: &garbage_id }),
                json!({ ODATA_ID: &hgx_id }),
            ]
        }),
    ));

    let systems = service_root.systems().await?.unwrap();
    bmc.expect(Expect::get(
        &dgx_id,
        computer_system(&ids, json!({ ODATA_ID: &dgx_id })),
    ));
    bmc.expect(Expect::get(
        &hgx_id,
        computer_system(&ids, json!({ ODATA_ID: &hgx_id })),
    ));

    let members = systems.members().await?;

    // Should only have DGX and HGX_Baseboard_0, not the garbage FDR entry
    assert_eq!(members.len(), 2);

    let member_ids: Vec<_> = members.iter().map(|m| m.odata_id().to_string()).collect();
    assert!(member_ids.contains(&dgx_id));
    assert!(member_ids.contains(&hgx_id));
    assert!(!member_ids.contains(&garbage_id));

    Ok(())
}

struct ComputerSystemIds {
    root_id: ODataId,
    systems_id: String,
    system_id: String,
}

fn computer_system_ids() -> ComputerSystemIds {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/System-1");
    ComputerSystemIds {
        root_id,
        systems_id,
        system_id,
    }
}

fn computer_system_ids_blue_field() -> ComputerSystemIds {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/Bluefield");
    ComputerSystemIds {
        root_id,
        systems_id,
        system_id,
    }
}

fn resource_name(id: &str) -> &str {
    id.rsplit('/').next().unwrap_or(id)
}

fn computer_system(ids: &ComputerSystemIds, fields: Value) -> Value {
    let override_id = fields
        .as_object()
        .and_then(|obj| obj.get(ODATA_ID))
        .and_then(Value::as_str);

    let system_id = override_id.unwrap_or(ids.system_id.as_str());
    let name = resource_name(system_id);
    let base = json!({
        ODATA_ID: system_id,
        ODATA_TYPE: &SYSTEM_DATA_TYPE,
        "Id": name,
        "Name": name,
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    });
    json_merge([&base, &fields])
}

async fn get_system(
    bmc: Arc<Bmc>,
    ids: &ComputerSystemIds,
    member: Value,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    let systems = get_systems(bmc, ids, "NVIDIA", vec![member]).await?;
    let mut members = systems.members().await?;
    members.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing computer system").into()
    })
}
