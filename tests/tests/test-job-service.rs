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

//! Integration tests for the standard Redfish JobService.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::schema::job::JobState;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
use nv_redfish_tests::{Bmc, Expect, ODATA_ID, ODATA_TYPE};
use serde_json::json;

#[tokio::test]
async fn job_service_follows_advertised_collection_and_job_links() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let service_id = "/redfish/v1/custom-job-service";
    let jobs_id = "/redfish/v1/custom-job-service/jobs";
    let job_id = "/redfish/v1/custom-job-service/jobs/JID_42";

    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: "#ServiceRoot.v1_19_0.ServiceRoot",
            "Id": "RootService",
            "Name": "Root service",
            "JobService": { ODATA_ID: service_id },
            "Links": {
                "Sessions": { ODATA_ID: "/redfish/v1/SessionService/Sessions" }
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;
    bmc.expect(Expect::get(
        service_id,
        json!({
            ODATA_ID: service_id,
            ODATA_TYPE: "#JobService.v1_1_0.JobService",
            "Id": "JobService",
            "Name": "Job service",
            "ServiceEnabled": true,
            "Jobs": { ODATA_ID: jobs_id }
        }),
    ));
    let service = root.job_service().await?.expect("JobService is advertised");
    let jobs = service.jobs().expect("Jobs is advertised");
    assert_eq!(jobs.odata_id().to_string(), jobs_id);

    bmc.expect(Expect::get(
        jobs_id,
        json!({
            ODATA_ID: jobs_id,
            ODATA_TYPE: "#JobCollection.JobCollection",
            "Name": "Jobs",
            "Members": [{ ODATA_ID: job_id }]
        }),
    ));
    let members = jobs.member_links().await?;
    assert_eq!(members.len(), 1);

    bmc.expect(Expect::get(
        job_id,
        json!({
            ODATA_ID: job_id,
            ODATA_TYPE: "#Job.v1_3_0.Job",
            "Id": "JID_42",
            "Name": "Configure BIOS",
            "JobState": "Completed",
            "JobStatus": "OK",
            "PercentComplete": 100,
            "StartTime": "2026-08-05T04:25:43+00:00"
        }),
    ));
    assert_eq!(
        members[0].fetch().await?.job_state,
        Some(JobState::Completed)
    );

    Ok(())
}
