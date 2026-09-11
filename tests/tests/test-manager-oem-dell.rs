// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Integration tests for Dell resources advertised by Manager OEM links.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::manager::Manager;
use nv_redfish::ServiceRoot;
use nv_redfish_core::{ModificationResponse, ODataId};
use nv_redfish_tests::{assert_empty, Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::{json, Value};

const SERVICE_ROOT_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const MANAGER_COLLECTION_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_TYPE: &str = "#Manager.v1_18_0.Manager";
const DELL_ATTRIBUTES_TYPE: &str = "#DellAttributes.v1_0_0.DellAttributes";
const DELL_JOB_SERVICE_TYPE: &str = "#DellJobService.v1_2_0.DellJobService";

#[tokio::test]
async fn manager_follows_advertised_dell_resources_and_actions() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let manager_id = "/redfish/v1/Managers/manager-1";
    let attributes_id = "/redfish/v1/vendor/dell/attributes/manager-1";
    let job_service_id = "/redfish/v1/vendor/dell/job-service";
    let delete_target = "/redfish/v1/vendor/dell/actions/delete";
    let manager = get_manager(
        bmc.clone(),
        manager_id,
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "DellAttributes": [{ ODATA_ID: attributes_id }],
                        "DellJobService": { ODATA_ID: job_service_id }
                    }
                }
            }
        }),
    )
    .await?;
    let dell = manager.oem_dell()?.expect("Dell links are advertised");

    bmc.expect(Expect::get(
        attributes_id,
        json!({
            ODATA_ID: attributes_id,
            ODATA_TYPE: DELL_ATTRIBUTES_TYPE,
            "Id": "manager-1",
            "Name": "Manager attributes",
            "Attributes": { "SSH.1.Enable": "Enabled" }
        }),
    ));
    let attributes = dell
        .manager_attributes()
        .await?
        .expect("manager attributes are advertised");
    assert!(attributes
        .attribute("SSH.1.Enable")
        .is_some_and(|value| value.str_value() == Some("Enabled")));
    bmc.expect(Expect::update_empty(
        attributes_id,
        json!({ "Attributes": { "SSH.1.Enable": "Disabled" } }),
    ));
    assert_empty(
        attributes
            .update(&json!({ "SSH.1.Enable": "Disabled" }))
            .await?,
    );

    bmc.expect(Expect::get(
        job_service_id,
        json!({
            ODATA_ID: job_service_id,
            ODATA_TYPE: DELL_JOB_SERVICE_TYPE,
            "Id": "JobService",
            "Name": "Dell job service",
            "Actions": {
                "#DellJobService.DeleteJobQueue": { "target": delete_target }
            }
        }),
    ));
    bmc.expect(Expect::action(
        delete_target,
        json!({ "JobID": "JID_CLEARALL" }),
        json!(null),
    ));
    let job_service = dell
        .job_service()
        .await?
        .expect("Dell job service is advertised");
    assert!(matches!(
        job_service.delete_job_queue("JID_CLEARALL").await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[tokio::test]
async fn manager_without_dell_links_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let manager = get_manager(
        bmc,
        "/redfish/v1/Managers/manager-1",
        json!({ "Links": {} }),
    )
    .await?;

    assert!(manager.oem_dell()?.is_none());

    Ok(())
}

#[tokio::test]
async fn manager_exposes_resource_oem_jobs_as_legacy_fallback() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let jobs_id = "/redfish/v1/Managers/1/Oem/Dell/Jobs";
    let manager = get_manager(
        bmc,
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "DellJobService": {
                            ODATA_ID: "/redfish/v1/Managers/1/Oem/Dell/DellJobService"
                        }
                    }
                }
            },
            "Oem": {
                "Dell": {
                    "Jobs": { ODATA_ID: jobs_id }
                }
            }
        }),
    )
    .await?;

    assert_eq!(
        manager
            .oem_dell()?
            .expect("Dell resources are advertised")
            .legacy_jobs()
            .expect("legacy Jobs link is advertised")
            .odata_id()
            .to_string(),
        jobs_id
    );

    Ok(())
}

async fn get_manager(
    bmc: Arc<Bmc>,
    manager_id: &str,
    extra: Value,
) -> Result<Manager<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    let managers_id = format!("{root_id}/Managers");
    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: SERVICE_ROOT_TYPE,
            "Id": "RootService",
            "Name": "Root service",
            "Managers": { ODATA_ID: &managers_id },
            "ProtocolFeaturesSupported": {
                "ExpandQuery": { "NoLinks": true }
            },
            "Links": {
                "Sessions": { ODATA_ID: format!("{root_id}/SessionService/Sessions") }
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;
    bmc.expect(Expect::expand(
        &managers_id,
        json!({
            ODATA_ID: &managers_id,
            ODATA_TYPE: MANAGER_COLLECTION_TYPE,
            "Id": "Managers",
            "Name": "Managers",
            "Members": [{
                ODATA_ID: manager_id,
                ODATA_TYPE: MANAGER_TYPE,
                "Id": "manager-1",
                "Name": "Manager",
                "Status": { "State": "Enabled" },
                "Links": extra["Links"].clone(),
                "Oem": extra["Oem"].clone()
            }]
        }),
    ));
    root.managers()
        .await?
        .expect("Managers is advertised")
        .members()
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "manager missing".into())
}
