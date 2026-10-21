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
//! Integration tests for HPE Manager OEM support.

use nv_redfish::manager::Manager;
use nv_redfish::oem::hpe::date_time::{HpeiLoDateTimeUpdate, TimeZoneUpdate};
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::assert_empty;
use nv_redfish_tests::assert_task;
use nv_redfish_tests::async_task;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use std::error::Error as StdError;
use std::sync::Arc;
use tokio::test;

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const MANAGER_COLLECTION_DATA_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_DATA_TYPE: &str = "#Manager.v1_16_0.Manager";
const HPE_ILO_DATA_TYPE: &str = "#HpeiLO.v2_11_0.HpeiLO";
const HPE_DATE_TIME_DATA_TYPE: &str = "#HpeiLODateTime.v2_0_0.HpeiLODateTime";

#[test]
async fn hpe_virtual_nic_enabled_supported() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload(&ids, Some(json!(true)))).await?;

    let hpe = manager.oem_hpe()?.unwrap();
    assert_eq!(hpe.virtual_nic_enabled(), Some(true));

    Ok(())
}

#[test]
async fn hpe_virtual_nic_is_typed_and_updatable() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload(&ids, Some(json!(true))),
            &json!({ "@odata.etag": "W/\"manager-etag\"" }),
        ]),
    )
    .await?;
    bmc.expect(Expect::update(
        &ids.manager_id,
        json!({ "Oem": { "Hpe": { "VirtualNICEnabled": false } } }),
        manager_payload(&ids, Some(json!(false))),
    ));

    let hpe = manager.oem_hpe()?.ok_or("HPE Manager extension missing")?;
    let ModificationResponse::Entity(updated) = hpe
        .set_virtual_nic_enabled(false)
        .await?
        .ok_or("VirtualNICEnabled missing")?
    else {
        return Err("expected updated Manager".into());
    };
    assert_eq!(
        updated
            .oem_hpe()?
            .and_then(|manager| manager.virtual_nic_enabled()),
        Some(false)
    );
    Ok(())
}

#[test]
async fn hpe_virtual_nic_update_requires_advertised_property() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc, &ids, manager_payload(&ids, None)).await?;

    assert!(manager
        .oem_hpe()?
        .ok_or("HPE Manager extension missing")?
        .set_virtual_nic_enabled(false)
        .await?
        .is_none());
    Ok(())
}

#[test]
async fn factory_reset_uses_nested_advertised_target() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let target = format!("{}/odd-target/factory-reset/", ids.manager_id);
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload(&ids, Some(json!(true))),
            &json!({
                "Oem": {
                    "Hpe": {
                        "Actions": {
                            "#HpeiLO.ResetToFactoryDefaults": {
                                "target": &target
                            }
                        }
                    }
                }
            }),
        ]),
    )
    .await?;
    bmc.expect(Expect::action(
        &target,
        json!({ "ResetType": "Default" }),
        json!(null),
    ));

    let hpe = manager.oem_hpe()?.ok_or("HPE Manager extension missing")?;
    assert!(matches!(
        hpe.reset_to_factory_defaults().await?,
        ModificationResponse::Entity(())
    ));
    Ok(())
}

#[test]
async fn factory_reset_requires_nested_action_advertisement() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc,
        &ids,
        json_merge([
            &manager_payload(&ids, Some(json!(true))),
            &json!({ "Oem": { "Hpe": { "Actions": {} } } }),
        ]),
    )
    .await?;
    let hpe = manager.oem_hpe()?.ok_or("HPE Manager extension missing")?;

    assert!(matches!(
        hpe.reset_to_factory_defaults().await,
        Err(nv_redfish::Error::ActionNotAvailable)
    ));
    Ok(())
}

#[test]
async fn hpe_date_time_follows_advertised_link_and_updates_ntp() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let date_time_id = format!("{}/custom-date-time/", ids.manager_id);
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload(&ids, Some(json!(true))),
            &json!({
                "Oem": {
                    "Hpe": {
                        "Links": {
                            "DateTimeService": { ODATA_ID: &date_time_id }
                        }
                    }
                }
            }),
        ]),
    )
    .await?;
    bmc.expect(Expect::get(
        &date_time_id,
        date_time_payload(&date_time_id, &["10.0.0.1", "10.0.0.2"], false, 15),
    ));

    let date_time = manager
        .oem_hpe()?
        .ok_or("HPE Manager extension missing")?
        .date_time()
        .await?
        .ok_or("HPE DateTime link missing")?;
    assert_eq!(
        date_time.static_ntp_servers(),
        Some(["10.0.0.1".to_string(), "10.0.0.2".to_string()].as_slice())
    );
    assert_eq!(date_time.propagate_time_to_host(), Some(false));
    assert_eq!(date_time.time_zone().and_then(|zone| zone.index), Some(15));

    let update = HpeiLoDateTimeUpdate::builder()
        .with_static_ntp_servers(vec!["10.0.0.3".into(), "10.0.0.4".into()])
        .with_propagate_time_to_host(true)
        .with_time_zone(TimeZoneUpdate::builder().with_index(16).build())
        .build();
    let request = json!({
        "PropagateTimeToHost": true,
        "StaticNTPServers": ["10.0.0.3", "10.0.0.4"],
        "TimeZone": { "Index": 16 }
    });
    bmc.expect(Expect::update(
        &date_time_id,
        &request,
        date_time_payload(&date_time_id, &["10.0.0.3", "10.0.0.4"], true, 16),
    ));
    let ModificationResponse::Entity(updated) = date_time.update(&update).await? else {
        return Err("expected updated HPE DateTime".into());
    };
    assert_eq!(
        updated.static_ntp_servers(),
        Some(["10.0.0.3".to_string(), "10.0.0.4".to_string()].as_slice())
    );
    assert_eq!(updated.propagate_time_to_host(), Some(true));
    assert_eq!(updated.time_zone().and_then(|zone| zone.index), Some(16));

    let task_id = "/redfish/v1/TaskService/Tasks/date-time-update";
    bmc.expect(Expect::update_task(
        &date_time_id,
        &request,
        async_task(task_id, 4),
    ));
    assert_task(updated.update(&update).await?, task_id, 4);
    bmc.expect(Expect::update_empty(&date_time_id, &request));
    assert_empty(updated.update(&update).await?);
    Ok(())
}

#[test]
async fn hpe_date_time_returns_none_when_link_is_absent() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc, &ids, manager_payload(&ids, Some(json!(true)))).await?;

    assert!(manager
        .oem_hpe()?
        .ok_or("HPE Manager extension missing")?
        .date_time()
        .await?
        .is_none());
    Ok(())
}

#[test]
async fn manager_without_hpe_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload_without_hpe(&ids)).await?;

    assert!(manager.oem_hpe()?.is_none());

    Ok(())
}

#[test]
async fn manager_with_null_hpe_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload_without_hpe(&ids),
            &json!({ "Oem": { "Hpe": null } }),
        ]),
    )
    .await?;

    assert!(manager.oem_hpe()?.is_none());

    Ok(())
}

#[test]
async fn malformed_hpe_oem_returns_parse_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        manager_payload(&ids, Some(json!("true"))),
    )
    .await?;

    let err = match manager.oem_hpe() {
        Ok(v) => panic!("expected parse error, got: {:?}", v.is_some()),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("invalid type"),
        "unexpected error: {}",
        err
    );

    Ok(())
}

async fn get_manager(
    bmc: Arc<Bmc>,
    ids: &Ids,
    manager: Value,
) -> Result<Manager<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.managers_id,
        json!({
            ODATA_ID: &ids.managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_DATA_TYPE,
            "Id": "Managers",
            "Name": "Manager Collection",
            "Members": [manager]
        }),
    ));

    let collection = root.managers().await?.unwrap();
    let members = collection.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single manager must exist"))
}

async fn expect_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
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
            "Managers": { ODATA_ID: &ids.managers_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct Ids {
    root_id: ODataId,
    managers_id: String,
    manager_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    let manager_id = format!("{managers_id}/1");
    Ids {
        root_id,
        managers_id,
        manager_id,
    }
}

fn manager_payload(ids: &Ids, virtual_nic_enabled: Option<Value>) -> Value {
    let base = json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "ManagerType": "BMC",
        "Status": { "State": "Enabled" },
    });

    let mut hpe = json!({
        ODATA_TYPE: HPE_ILO_DATA_TYPE,
    });
    if let Some(v) = virtual_nic_enabled {
        hpe["VirtualNICEnabled"] = v;
    }

    let oem = json!({
        "Oem": {
            "Hpe": hpe
        }
    });
    json_merge([&base, &oem])
}

fn manager_payload_without_hpe(ids: &Ids) -> Value {
    json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "1",
        "Name": "Manager",
        "ManagerType": "BMC",
        "Status": { "State": "Enabled" },
        "Oem": {}
    })
}

fn date_time_payload(
    id: &str,
    static_ntp_servers: &[&str],
    propagate_time_to_host: bool,
    time_zone_index: i64,
) -> Value {
    json!({
        ODATA_ID: id,
        ODATA_TYPE: HPE_DATE_TIME_DATA_TYPE,
        "@odata.etag": "W/\"date-time-etag\"",
        "Id": "DateTime",
        "Name": "iLO Date and Time Settings",
        "ConfigurationSettings": "Current",
        "DateTime": "2026-09-25T18:39:52Z",
        "NTPServers": static_ntp_servers,
        "PropagateTimeToHost": propagate_time_to_host,
        "StaticNTPServers": static_ntp_servers,
        "TimeZone": {
            "Index": time_zone_index,
            "Name": "UTC",
            "UtcOffset": "+00:00",
            "Value": "GMT-0"
        }
    })
}
