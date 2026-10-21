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

//! Integration tests for Dell resources advertised by Manager OEM links.

use std::collections::HashMap;
use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::manager::Manager;
use nv_redfish::oem::dell::attributes::{AttributesUpdate, DellAttributesUpdate};
use nv_redfish::oem::dell::schema::dell_job::{DellJob, JobState, JobType};
use nv_redfish::oem::dell::schema::dell_lc_service::GetRemoteServicesApiStatusResponseLcStatus;
use nv_redfish::oem::dell::schema::oem_manager::{
    ManagerImportSystemConfigurationAction, ShareParametersUpdate, ShutdownType,
};
use nv_redfish::oem::dell::schema::ActionAnnotations;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use nv_redfish_core::{AsyncTask, EdmPrimitiveType, ModificationResponse, ODataId};
use nv_redfish_tests::{assert_empty, json_merge, Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::{json, Value};

const SERVICE_ROOT_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const MANAGER_COLLECTION_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_TYPE: &str = "#Manager.v1_18_0.Manager";
const DELL_ATTRIBUTES_TYPE: &str = "#DellAttributes.v1_0_0.DellAttributes";
const DELL_JOB_SERVICE_TYPE: &str = "#DellJobService.v1_2_0.DellJobService";

#[tokio::test]
async fn manager_discovers_and_updates_advertised_dell_attributes() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let manager_id = "/redfish/v1/Managers/manager-1";
    let attributes_id = "/redfish/v1/vendor/dell/attributes/manager-1";
    let manager = get_manager(
        bmc.clone(),
        manager_id,
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "DellAttributes": [{ ODATA_ID: attributes_id }]
                    }
                }
            }
        }),
    )
    .await?;

    bmc.expect(Expect::get(
        attributes_id,
        json!({
            ODATA_ID: attributes_id,
            "@odata.etag": "W/\"attributes-1\"",
            ODATA_TYPE: DELL_ATTRIBUTES_TYPE,
            "Id": "manager-1",
            "Name": "Manager attributes",
            "Attributes": { "SSH.1.Enable": "Enabled" }
        }),
    ));
    let attributes = manager
        .oem_dell_attributes()
        .await?
        .expect("manager attributes are advertised");
    assert!(attributes
        .attribute("SSH.1.Enable")
        .is_some_and(|value| value.str_value() == Some("Enabled")));
    bmc.expect(Expect::update_empty(
        attributes_id,
        json!({ "Attributes": { "SSH.1.Enable": "Disabled" } }),
    ));
    let values = HashMap::from([(
        "SSH.1.Enable".to_string(),
        Some(EdmPrimitiveType::String("Disabled".to_string())),
    )]);
    let update = DellAttributesUpdate::builder()
        .with_attributes(
            AttributesUpdate::builder()
                .with_dynamic_properties(values)
                .build(),
        )
        .build();
    let update_debug = format!("{update:?}");
    assert!(!update_debug.contains("Disabled"));
    assert!(update_debug.contains("dynamic_properties: \"<redacted>\""));
    assert_empty(attributes.update(&update).await?);

    let fallback_bmc = Arc::new(Bmc::default());
    let fallback_manager = get_manager(
        fallback_bmc.clone(),
        manager_id,
        json!({
            "Links": {},
            "Oem": { "Dell": {} }
        }),
    )
    .await?;
    let fallback_id = format!("{manager_id}/Oem/Dell/DellAttributes/manager-1");
    fallback_bmc.expect(Expect::expand(
        &fallback_id,
        json!({
            ODATA_ID: &fallback_id,
            ODATA_TYPE: DELL_ATTRIBUTES_TYPE,
            "Id": "manager-1",
            "Name": "Manager attributes",
            "Attributes": {}
        }),
    ));
    assert!(fallback_manager.oem_dell_attributes().await?.is_some());

    Ok(())
}

#[tokio::test]
async fn manager_invokes_advertised_dell_job_queue_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let job_service_id = "/redfish/v1/vendor/dell/job-service";
    let delete_target = "/redfish/v1/vendor/dell/actions/delete";
    let manager = get_manager(
        bmc.clone(),
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "DellJobService": { ODATA_ID: job_service_id }
                    }
                }
            }
        }),
    )
    .await?;
    let dell = manager.oem_dell()?.expect("Dell links are advertised");

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
async fn manager_without_dell_extension_returns_none() -> Result<(), Box<dyn StdError>> {
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
async fn dell_job_service_without_usable_actions_reports_unavailable(
) -> Result<(), Box<dyn StdError>> {
    for actions in [None, Some(Value::Null), Some(json!({}))] {
        let has_empty_container =
            matches!(actions.as_ref(), Some(Value::Object(values)) if values.is_empty());
        let bmc = Arc::new(Bmc::default());
        let job_service_id = "/redfish/v1/vendor/dell/job-service";
        let manager = get_manager(
            bmc.clone(),
            "/redfish/v1/Managers/manager-1",
            json!({
                "Links": {
                    "Oem": {
                        "Dell": {
                            "DellJobService": { ODATA_ID: job_service_id }
                        }
                    }
                }
            }),
        )
        .await?;
        let mut service = json!({
            ODATA_ID: job_service_id,
            ODATA_TYPE: DELL_JOB_SERVICE_TYPE,
            "Id": "JobService",
            "Name": "Dell job service"
        });
        if let Some(actions) = actions {
            service["Actions"] = actions;
        }
        bmc.expect(Expect::get(job_service_id, service));

        let job_service = manager
            .oem_dell()?
            .expect("Dell links are advertised")
            .job_service()
            .await?
            .expect("Dell job service is advertised");
        let result = job_service.delete_job_queue("JID_CLEARALL").await;
        if has_empty_container {
            assert!(matches!(result, Err(nv_redfish::Error::Bmc(_))));
        } else {
            assert!(matches!(result, Err(nv_redfish::Error::ActionNotAvailable)));
        }
    }

    Ok(())
}

#[tokio::test]
async fn manager_top_level_configuration_job_links_to_its_dell_job() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let jobs_id = "/redfish/v1/Managers/1/Oem/Dell/Jobs";
    let manager = get_manager(
        bmc.clone(),
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {},
            "Oem": {
                "Dell": {
                    "Jobs": { ODATA_ID: jobs_id }
                }
            }
        }),
    )
    .await?;

    let jobs = manager
        .oem_dell()?
        .expect("Dell resources are advertised")
        .configuration_jobs()
        .expect("configuration Jobs link is advertised");
    assert_eq!(jobs.odata_id().to_string(), jobs_id);

    let settings_id = ODataId::from("/redfish/v1/Systems/1/Bios/Settings".to_string());
    let task_id = "/redfish/v1/Managers/1/Oem/Dell/Jobs/JID_43";
    bmc.expect(Expect::create(
        jobs_id,
        json!({ "TargetSettingsURI": settings_id }),
        json!({ ODATA_ID: task_id }),
    ));
    let ModificationResponse::Task(task) = jobs.create_configuration_job(&settings_id).await?
    else {
        panic!("expected asynchronous Dell job");
    };
    assert_eq!(task.location.0.to_string(), task_id);
    assert_eq!(task.retry_after, None);

    let link = jobs.job_link(task)?;
    bmc.expect(Expect::get(
        task_id,
        json!({
            ODATA_ID: task_id,
            ODATA_TYPE: "#DellJob.v1_5_0.DellJob",
            "Id": "JID_43",
            "Name": "Configure: BIOS.Setup.1-1",
            "JobState": "Scheduled",
            "MessageId": "JCP001",
            "PercentComplete": 0
        }),
    ));
    assert_eq!(
        link.fetch().await?.job_state,
        Some(Some(JobState::Scheduled))
    );

    let outside = AsyncTask {
        location: ODataId::from("/redfish/v1/TaskService/Tasks/JID_43".to_string()).into(),
        retry_after: None,
        task: None,
    };
    assert!(matches!(
        jobs.job_link(outside),
        Err(Error::JobLocationNotInJobs { .. })
    ));

    Ok(())
}

#[test]
fn dell_job_reports_state_on_idrac9_and_idrac10() -> Result<(), Box<dyn StdError>> {
    // iDRAC9 7.20.10.50: a BIOS configuration job waiting for a host reset.
    let idrac9: DellJob = serde_json::from_value(json!({
        ODATA_ID: "/redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/Jobs/JID_907777700987",
        ODATA_TYPE: "#DellJob.v1_5_0.DellJob",
        "ActualRunningStartTime": null,
        "ActualRunningStopTime": null,
        "CompletionTime": null,
        "Description": "Job Instance",
        "EndTime": "TIME_NA",
        "Id": "JID_907777700987",
        "JobState": "Scheduled",
        "JobType": "BIOSConfiguration",
        "Message": "Task successfully scheduled.",
        "MessageArgs": [],
        "MessageArgs@odata.count": 0,
        "MessageId": "JCP001",
        "Name": "Configure: BIOS.Setup.1-1",
        "PercentComplete": 0,
        "StartTime": "2026-09-30T09:16:10",
        "TargetSettingsURI": null
    }))?;
    assert_eq!(idrac9.job_state, Some(Some(JobState::Scheduled)));
    assert_eq!(idrac9.job_type, Some(Some(JobType::BiosConfiguration)));
    assert_eq!(idrac9.message_id, Some(Some("JCP001".to_string())));
    assert_eq!(idrac9.percent_complete, Some(0));

    // iDRAC10 1.30.30.52 reports the same wait as ReadyForExecution.
    let idrac10: DellJob = serde_json::from_value(json!({
        ODATA_ID: "/redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/Jobs/JID_179077943563",
        ODATA_TYPE: "#DellJob.v1_8_0.DellJob",
        "Id": "JID_179077943563",
        "Name": "Configure: BIOS.Setup.1-1",
        "JobState": "ReadyForExecution",
        "JobType": "BIOSConfiguration",
        "Message": "Configuration changes committed",
        "MessageId": "PR19",
        "PercentComplete": 0
    }))?;
    assert_eq!(idrac10.job_state, Some(Some(JobState::ReadyForExecution)));

    Ok(())
}

#[tokio::test]
async fn manager_follows_configuration_jobs_link() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let jobs_id = "/redfish/v1/Managers/1/Oem/Dell/Jobs";
    let manager = get_manager(
        bmc,
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "Jobs": { ODATA_ID: jobs_id }
                    }
                }
            }
        }),
    )
    .await?;

    let jobs = manager
        .oem_dell()?
        .expect("Dell resources are advertised")
        .configuration_jobs()
        .expect("configuration Jobs link is advertised");
    assert_eq!(jobs.odata_id().to_string(), jobs_id);

    Ok(())
}

#[tokio::test]
async fn manager_prefers_configuration_jobs_link_over_top_level_oem(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let linked_jobs_id = "/redfish/v1/Managers/1/Oem/Dell/Jobs";
    let top_level_jobs_id = "/redfish/v1/Managers/1/Jobs";
    let manager = get_manager(
        bmc,
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "Jobs": { ODATA_ID: linked_jobs_id }
                    }
                }
            },
            "Oem": {
                "Dell": {
                    "Jobs": { ODATA_ID: top_level_jobs_id }
                }
            }
        }),
    )
    .await?;

    let jobs = manager
        .oem_dell()?
        .expect("Dell resources are advertised")
        .configuration_jobs()
        .expect("configuration Jobs link is advertised");
    assert_eq!(jobs.odata_id().to_string(), linked_jobs_id);

    Ok(())
}

#[tokio::test]
async fn manager_imports_system_configuration() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let target =
        "/redfish/v1/Managers/iDRAC.Embedded.1/Actions/Oem/EID_674_Manager.ImportSystemConfiguration";
    let manager = get_manager(
        bmc.clone(),
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {},
            "Actions": {
                "Oem": {
                    "#OemManager.ImportSystemConfiguration": { "target": target },
                    "#OemManager.ExportSystemConfiguration": {
                        "target": "/redfish/v1/Managers/iDRAC.Embedded.1/Actions/Oem/EID_674_Manager.ExportSystemConfiguration"
                    }
                }
            }
        }),
    )
    .await?;
    let dell = manager.oem_dell()?.expect("Dell actions are advertised");

    bmc.expect(Expect::action_task(
        target,
        json!({
            "ShareParameters": { "Target": ["BIOS"] },
            "ImportBuffer": "<SystemConfiguration/>",
            "ShutdownType": "Forced"
        }),
        AsyncTask {
            location: ODataId::from("/redfish/v1/TaskService/Tasks/JID_1".to_string()).into(),
            retry_after: None,
            task: None,
        },
    ));
    let params = ManagerImportSystemConfigurationAction {
        redfish_annotations: ActionAnnotations::default(),
        share_parameters: ShareParametersUpdate::builder()
            .with_target(vec!["BIOS".to_string()])
            .build(),
        import_buffer: Some("<SystemConfiguration/>".to_string()),
        shutdown_type: Some(ShutdownType::Forced),
        host_power_state: None,
    };
    let ModificationResponse::Task(task) = dell.import_system_configuration(&params).await? else {
        return Err("expected the import to return a job".into());
    };
    assert_eq!(
        task.location.0.to_string(),
        "/redfish/v1/TaskService/Tasks/JID_1"
    );

    Ok(())
}

#[tokio::test]
async fn manager_reads_lifecycle_controller_status() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let lc_id = "/redfish/v1/Managers/iDRAC.Embedded.1/Oem/Dell/DellLCService";
    let target = format!("{lc_id}/Actions/DellLCService.GetRemoteServicesAPIStatus");
    let manager = get_manager(
        bmc.clone(),
        "/redfish/v1/Managers/manager-1",
        json!({
            "Links": {
                "Oem": {
                    "Dell": {
                        "DellLCService": { ODATA_ID: lc_id }
                    }
                }
            }
        }),
    )
    .await?;
    let dell = manager.oem_dell()?.expect("Dell links are advertised");

    bmc.expect(Expect::get(
        lc_id,
        json!({
            ODATA_ID: lc_id,
            ODATA_TYPE: "#DellLCService.v1_8_1.DellLCService",
            "Id": "DellLCService",
            "Name": "DellLCService",
            "Actions": {
                "#DellLCService.GetRemoteServicesAPIStatus": { "target": &target }
            }
        }),
    ));
    let lc = dell.lc_service().await?.expect("DellLCService is linked");

    // iDRAC9 reports TelemetryServiceStatus.
    bmc.expect(Expect::action(
        &target,
        json!({}),
        json!({
            "LCStatus": "Ready",
            "RTStatus": "Ready",
            "SEKMServiceStatus": "NotReady",
            "ServerStatus": "OutOfPOST",
            "Status": "Ready",
            "TelemetryServiceStatus": "Ready"
        }),
    ));
    let ModificationResponse::Entity(status) = lc.remote_services_api_status().await? else {
        return Err("expected a status body".into());
    };
    assert_eq!(
        status.lc_status,
        Some(GetRemoteServicesApiStatusResponseLcStatus::Ready)
    );
    assert_eq!(status.server_status.as_deref(), Some("OutOfPOST"));

    // iDRAC10 reports RedfishStatus instead, and ServerStatus values outside
    // any enum.
    bmc.expect(Expect::action(
        &target,
        json!({}),
        json!({
            "LCStatus": "InUse",
            "RTStatus": "Ready",
            "SEKMServiceStatus": "NotReady",
            "ServerStatus": "HaltedF1/F2Prompt",
            "Status": "InUse",
            "RedfishStatus": "Ready"
        }),
    ));
    let ModificationResponse::Entity(status) = lc.remote_services_api_status().await? else {
        return Err("expected a status body".into());
    };
    assert_eq!(
        status.lc_status,
        Some(GetRemoteServicesApiStatusResponseLcStatus::InUse)
    );
    assert!(status.telemetry_service_status.is_none());

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
            "Members": [json_merge([
                &json!({
                    ODATA_ID: manager_id,
                    ODATA_TYPE: MANAGER_TYPE,
                    "Id": "manager-1",
                    "Name": "Manager",
                    "Status": { "State": "Enabled" }
                }),
                &extra,
            ])]
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
