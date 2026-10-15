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
//! Integration tests for Chassis collection workaround behavior.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::chassis::Chassis;
use nv_redfish::chassis::PowerSupply;
use nv_redfish::control::ControlUpdate;
use nv_redfish::resource::ResetType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::ami_service_root;
use nv_redfish_tests::ami_viking_service_root;
use nv_redfish_tests::anonymous_1_9_service_root;
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

const CHASSIS_COLLECTION_DATA_TYPE: &str = "#ChassisCollection.ChassisCollection";
const CHASSIS_DATA_TYPE: &str = "#Chassis.v1_23_0.Chassis";
const POWER_SUBSYSTEM_DATA_TYPE: &str = "#PowerSubsystem.v1_1_0.PowerSubsystem";
const POWER_SUPPLY_COLLECTION_DATA_TYPE: &str = "#PowerSupplyCollection.PowerSupplyCollection";
const POWER_SUPPLY_DATA_TYPE: &str = "#PowerSupply.v1_5_0.PowerSupply";

#[test]
async fn reset_invokes_chassis_reset_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let action_target = format!("{}/Actions/Chassis.Reset", ids.chassis_id);
    let chassis = get_chassis(
        bmc.clone(),
        &ids,
        chassis_payload(
            &ids,
            redfish_action_payload("Chassis.Reset", &action_target),
        ),
    )
    .await?;

    expect_redfish_reset_action(&bmc, &action_target, Some("ForceRestart"));

    assert!(matches!(
        chassis.reset(Some(ResetType::ForceRestart)).await?,
        ModificationResponse::Entity(())
    ));

    expect_redfish_reset_action(&bmc, &action_target, None);

    assert!(matches!(
        chassis.reset(None).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn reset_returns_action_not_available_when_chassis_reset_is_absent(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let chassis = get_chassis(
        bmc.clone(),
        &ids,
        chassis_payload(&ids, redfish_empty_actions_payload()),
    )
    .await?;

    assert!(matches!(
        chassis.reset(Some(ResetType::ForceRestart)).await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));

    Ok(())
}

#[test]
async fn reset_invokes_power_supply_reset_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let power_ids = power_supply_ids(&ids);
    let action_target = format!("{}/Actions/PowerSupply.Reset", power_ids.power_supply_id);
    let power_supply = get_power_supply(
        bmc.clone(),
        &ids,
        &power_ids,
        redfish_action_payload("PowerSupply.Reset", &action_target),
    )
    .await?;

    expect_redfish_reset_action(&bmc, &action_target, Some("GracefulRestart"));

    assert!(matches!(
        power_supply.reset(Some(ResetType::GracefulRestart)).await?,
        ModificationResponse::Entity(())
    ));

    expect_redfish_reset_action(&bmc, &action_target, None);

    assert!(matches!(
        power_supply.reset(None).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn reset_returns_action_not_available_when_power_supply_reset_is_absent(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let power_ids = power_supply_ids(&ids);
    let power_supply =
        get_power_supply(bmc, &ids, &power_ids, redfish_empty_actions_payload()).await?;

    assert!(matches!(
        power_supply.reset(Some(ResetType::GracefulRestart)).await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));

    Ok(())
}

#[test]
async fn ami_viking_missing_root_chassis_nav_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_viking_service_root(bmc.clone(), &ids, json!({})).await?;
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(bmc.clone(), &ids, valid_chassis_payload(&ids));
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn environment_power_limit_control_fetches_and_updates() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let metrics_id = format!("{}/EnvironmentMetrics", ids.chassis_id);
    let control_id = format!("{}/Controls/PowerLimit", ids.chassis_id);
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), &ids);
    let Some(collection) = root.chassis().await? else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing chassis collection",
        )
        .into());
    };

    expect_chassis_get(
        bmc.clone(),
        &ids,
        json_merge([
            &valid_chassis_payload(&ids),
            &json!({
                "EnvironmentMetrics": {
                    ODATA_ID: &metrics_id
                }
            }),
        ]),
    );
    let mut members = collection.members().await?;
    let Some(chassis) = members.pop() else {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "missing chassis").into());
    };

    bmc.expect(Expect::get(
        &metrics_id,
        json!({
            ODATA_ID: &metrics_id,
            ODATA_TYPE: "#EnvironmentMetrics.v1_1_0.EnvironmentMetrics",
            "Id": "EnvironmentMetrics",
            "Name": "Environment Metrics",
            "PowerLimitWatts": {
                "DataSourceUri": &control_id,
                "SetPoint": 600.0
            }
        }),
    ));
    bmc.expect(Expect::get(
        &control_id,
        control_payload(&control_id, 600.0),
    ));
    let Some(power_limit) = chassis.environment_power_limit_control().await? else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing power limit control",
        )
        .into());
    };

    assert_eq!(power_limit.raw().set_point, Some(Some(600.0)));
    assert_eq!(power_limit.raw().allowable_min, Some(Some(400.0)));
    assert_eq!(power_limit.raw().allowable_max, Some(Some(900.0)));

    let update = ControlUpdate::builder().with_set_point(700.0).build();
    let update_json = serde_json::to_value(&update)?;
    bmc.expect(Expect::update(
        &control_id,
        update_json,
        control_payload(&control_id, 700.0),
    ));
    let ModificationResponse::Entity(updated_power_limit) = power_limit.update(&update).await?
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing updated power limit control",
        )
        .into());
    };

    assert_eq!(updated_power_limit.raw().set_point, Some(Some(700.0)));

    Ok(())
}

#[test]
async fn ami_viking_invalid_contained_by_fields_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_viking_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(
        bmc.clone(),
        &ids,
        json!({
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "1",
            "Name": "Chassis",
            "ChassisType": "RackMount",
            "Links": {
                "ContainedBy": {
                    ODATA_ID: &ids.container_chassis_id,
                    "InvalidField": "invalid"
                }
            }
        }),
    );
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn ami_viking_missing_chassis_type_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_viking_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(
        bmc.clone(),
        &ids,
        json!({
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "1",
            "Name": "Chassis"
        }),
    );
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn ami_viking_missing_chassis_name_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_viking_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(
        bmc.clone(),
        &ids,
        json!({
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "1",
            "ChassisType": "RackMount"
        }),
    );
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn ami_gb300_disables_expand_for_chassis_collection() -> Result<(), Box<dyn StdError>> {
    // Platform under test: Grace-based NVIDIA GB300 host BMC (AMI, RtpVersion 13.09.1).
    // Quirk under test: its `$expand` drops Required fields (Id/Name/ChassisType)
    // from embedded members, so the collection is fetched with a plain GET and
    // members are fetched individually (complete) rather than via `$expand`.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_gb300_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    // A plain (non-`$expand`) collection GET; if the quirk did not apply the
    // collection would be fetched via `$expand` and this expectation would not
    // match.
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(bmc.clone(), &ids, valid_chassis_payload(&ids));
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn ami_without_gb300_rtp_version_uses_expand() -> Result<(), Box<dyn StdError>> {
    // A non-GB300 AMI BMC (no GB300 `RtpVersion`) must NOT be penalized: it keeps
    // using `$expand` for collections.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_generic_ami_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    // Collection fetched via `$expand` with the member inline.
    bmc.expect(Expect::expand(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [valid_chassis_payload(&ids)]
        }),
    ));

    let collection = root.chassis().await?.unwrap();
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn nvidia_gb300_disables_expand_for_chassis_collection() -> Result<(), Box<dyn StdError>> {
    // Platform under test: GB300 host BMC on NVIDIA firmware (`GB BMC`),
    // whose service root advertises `$expand`.
    // Quirk under test: as on the AMI GB300, `$expand` is not used, so the
    // collection is fetched with a plain GET and members individually, while
    // the quirks the class shares with other NVIDIA BMCs still apply.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_nvidia_gb300_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(bmc.clone(), &ids, valid_chassis_payload(&ids));
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn anonymous_1_9_0_wrong_chassis_status_state_workaround() -> Result<(), Box<dyn StdError>> {
    // Platform under test: Liteon powershelf class (anonymous Redfish 1.9.0 root).
    // Quirk under test: invalid Chassis.Status.State="Standby".
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), &ids);

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(
        bmc.clone(),
        &ids,
        json!({
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "1",
            "Name": "Chassis",
            "ChassisType": "Shelf",
            "Status": {
                "Health": "OK",
                "HealthRollup": "OK",
                "State": "Standby"
            }
        }),
    );
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

#[test]
async fn nvidia_dpu_empty_chassis_uuid_in_expanded_members_workaround(
) -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVIDIA DPU.
    // Quirk under test: Sometimes Chassis.UUID="" in inline
    // collection members when DPU is in NIC mode.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_nvidia_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    bmc.expect(Expect::expand(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [
                {
                    ODATA_ID: &ids.chassis_id,
                    ODATA_TYPE: CHASSIS_DATA_TYPE,
                    "Id": "1",
                    "Name": "Chassis",
                    "ChassisType": "RackMount",
                    "UUID": ""
                }
            ]
        }),
    ));

    let collection = root.chassis().await?.unwrap();
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].raw().uuid, Some(None));

    Ok(())
}

#[test]
async fn nvidia_dpu_empty_chassis_uuid_on_member_fetch_workaround() -> Result<(), Box<dyn StdError>>
{
    // Platform under test: NVIDIA DPU.
    // Quirk under test: Sometimes Chassis.UUID="" in member payload
    // fetched by link.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_nvidia_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    bmc.expect(Expect::expand(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [
                {
                    ODATA_ID: &ids.chassis_id
                }
            ]
        }),
    ));

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(
        bmc.clone(),
        &ids,
        json!({
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "1",
            "Name": "Chassis",
            "ChassisType": "RackMount",
            "UUID": ""
        }),
    );
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].raw().uuid, Some(None));

    Ok(())
}

#[test]
async fn nvswitch_wrong_location_part_location_type_workaround() -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVSwitch (`Vendor=NVIDIA`, `Product=P3809`).
    // Quirk under test: invalid Location.PartLocation.LocationType="Unknown".
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let root = expect_nvswitch_service_root(
        bmc.clone(),
        &ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    bmc.expect(Expect::expand(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [
                {
                    ODATA_ID: &ids.chassis_id
                }
            ]
        }),
    ));

    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(
        bmc.clone(),
        &ids,
        json!({ // Real id: CPLD_0
            ODATA_ID: &ids.chassis_id,
            ODATA_TYPE: CHASSIS_DATA_TYPE,
            "Id": "1",
            "Name": "Chassis",
            "ChassisType": "Module",
            "Location": {
                "PartLocation": {
                    "LocationType": "Unknown"
                }
            }
        }),
    );
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);

    Ok(())
}

async fn expect_viking_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        ami_viking_service_root(&ids.root_id, fields),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_gb300_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        ami_service_root(&ids.root_id, "1.21.1", Some("13.09.1"), fields),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_generic_ami_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        ami_service_root(&ids.root_id, "1.21.1", None, fields),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
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

async fn expect_nvidia_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json_merge([
            &json!({
                ODATA_ID: &ids.root_id,
                ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
                "Id": "RootService",
                "Name": "RootService",
                "Vendor": "Nvidia",
                "Product": "Nvidia-BMCMezz",
                "ProtocolFeaturesSupported": {
                    "ExpandQuery": {
                        "NoLinks": true
                    }
                },
                "Links": {
                    "Sessions": {
                        ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                    }
                },
            }),
            &fields,
        ]),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_nvidia_gb300_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json_merge([
            &json!({
                ODATA_ID: &ids.root_id,
                ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
                "Id": "RootService",
                "Name": "RootService",
                "Vendor": "NVIDIA",
                "Product": "GB BMC",
                "ProtocolFeaturesSupported": {
                    "ExpandQuery": {
                        "NoLinks": true
                    }
                },
                "Links": {
                    "Sessions": {
                        ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                    }
                },
            }),
            &fields,
        ]),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

async fn expect_nvswitch_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    fields: Value,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json_merge([
            &json!({
                ODATA_ID: &ids.root_id,
                ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
                "Id": "RootService",
                "Name": "RootService",
                "Vendor": "NVIDIA",
                "Product": "P3809",
                "ProtocolFeaturesSupported": {
                    "ExpandQuery": {
                        "NoLinks": true
                    }
                },
                "Links": {
                    "Sessions": {
                        ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                    }
                },
            }),
            &fields,
        ]),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

fn expect_chassis_collection(bmc: Arc<Bmc>, ids: &Ids) {
    bmc.expect(Expect::get(
        &ids.chassis_collection_id,
        json!({
            ODATA_ID: &ids.chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [
                {
                    ODATA_ID: &ids.chassis_id
                }
            ]
        }),
    ));
}

fn expect_chassis_get(bmc: Arc<Bmc>, ids: &Ids, payload: Value) {
    bmc.expect(Expect::get(&ids.chassis_id, payload));
}

fn valid_chassis_payload(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.chassis_id,
        ODATA_TYPE: CHASSIS_DATA_TYPE,
        "Id": "1",
        "Name": "Chassis",
        "ChassisType": "RackMount"
    })
}

fn chassis_payload(ids: &Ids, fields: Value) -> Value {
    json_merge([&valid_chassis_payload(ids), &fields])
}

fn control_payload(control_id: &str, set_point: f64) -> Value {
    json!({
        ODATA_ID: control_id,
        ODATA_TYPE: "#Control.v1_7_0.Control",
        "Id": "PowerLimit",
        "Name": "Power Limit",
        "ControlType": "Power",
        "SetPointType": "Single",
        "SetPoint": set_point,
        "SetPointUnits": "W",
        "AllowableMin": 400.0,
        "AllowableMax": 900.0
    })
}

async fn get_chassis(
    bmc: Arc<Bmc>,
    ids: &Ids,
    payload: Value,
) -> Result<Chassis<Bmc>, Box<dyn StdError>> {
    let root = expect_anonymous_1_9_service_root(
        bmc.clone(),
        ids,
        json!({
            "Chassis": { ODATA_ID: &ids.chassis_collection_id }
        }),
    )
    .await?;
    expect_chassis_collection(bmc.clone(), ids);
    let collection = root.chassis().await?.unwrap();
    expect_chassis_get(bmc, ids, payload);
    let mut members = collection.members().await?;
    members.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing chassis").into()
    })
}

async fn get_power_supply(
    bmc: Arc<Bmc>,
    ids: &Ids,
    power_ids: &PowerSupplyIds,
    power_supply_fields: Value,
) -> Result<PowerSupply<Bmc>, Box<dyn StdError>> {
    let chassis = get_chassis(
        bmc.clone(),
        ids,
        chassis_payload(
            ids,
            json!({
                "PowerSubsystem": {
                    ODATA_ID: &power_ids.power_subsystem_id
                }
            }),
        ),
    )
    .await?;

    expect_power_supply(
        bmc,
        power_ids,
        power_supply_payload(power_ids, power_supply_fields),
    );
    let mut power_supplies = chassis.power_supplies().await?;
    power_supplies.pop().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, "missing power supply").into()
    })
}

struct PowerSupplyIds {
    power_subsystem_id: String,
    power_supply_collection_id: String,
    power_supply_id: String,
}

fn power_supply_ids(ids: &Ids) -> PowerSupplyIds {
    let power_subsystem_id = format!("{}/PowerSubsystem", ids.chassis_id);
    let power_supply_collection_id = format!("{power_subsystem_id}/PowerSupplies");
    let power_supply_id = format!("{power_supply_collection_id}/1");
    PowerSupplyIds {
        power_subsystem_id,
        power_supply_collection_id,
        power_supply_id,
    }
}

fn expect_power_supply(bmc: Arc<Bmc>, ids: &PowerSupplyIds, payload: Value) {
    bmc.expect(Expect::get(
        &ids.power_subsystem_id,
        json!({
            ODATA_ID: &ids.power_subsystem_id,
            ODATA_TYPE: POWER_SUBSYSTEM_DATA_TYPE,
            "Id": "PowerSubsystem",
            "Name": "Power Subsystem",
            "PowerSupplies": {
                ODATA_ID: &ids.power_supply_collection_id
            }
        }),
    ));
    bmc.expect(Expect::get(
        &ids.power_supply_collection_id,
        json!({
            ODATA_ID: &ids.power_supply_collection_id,
            ODATA_TYPE: POWER_SUPPLY_COLLECTION_DATA_TYPE,
            "Id": "PowerSupplies",
            "Name": "Power Supply Collection",
            "Members": [
                {
                    ODATA_ID: &ids.power_supply_id
                }
            ]
        }),
    ));
    bmc.expect(Expect::get(&ids.power_supply_id, payload));
}

fn power_supply_payload(ids: &PowerSupplyIds, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: &ids.power_supply_id,
        ODATA_TYPE: POWER_SUPPLY_DATA_TYPE,
        "Id": "1",
        "Name": "Power Supply 1",
        "Manufacturer": "NVIDIA",
        "Model": "PSU-1",
        "PowerState": true,
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    });
    json_merge([&base, &fields])
}

struct Ids {
    root_id: ODataId,
    chassis_collection_id: String,
    chassis_id: String,
    container_chassis_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let chassis_collection_id = format!("{root_id}/Chassis");
    let chassis_id = format!("{chassis_collection_id}/1");
    let container_chassis_id = format!("{chassis_collection_id}/0");
    Ids {
        root_id,
        chassis_collection_id,
        chassis_id,
        container_chassis_id,
    }
}
