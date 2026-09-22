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

//! This is tests support lib.

/// Schema compiled for base tests.
pub mod base;
/// Errors used in tests.
pub mod error;
/// Expectations in tests.
pub mod json_merge;

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use json_merge::json_merge;

/// Used in tests for `@odata.id` fields.
pub const ODATA_ID: &str = "@odata.id";
/// Used in tests for `@odata.type` fields.
pub const ODATA_TYPE: &str = "@odata.type";

use std::time::Duration;

use error::TestError;

use nv_redfish_bmc_mock::Bmc as MockBmc;
use nv_redfish_bmc_mock::Expect as MockExpect;
use nv_redfish_core::AsyncTask;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;

use serde_json::json;
use serde_json::Value;

pub type Bmc = MockBmc<TestError>;
pub type Expect = MockExpect<TestError>;

pub fn async_task(location: &str, retry_after_secs: u64) -> AsyncTask {
    AsyncTask {
        location: ODataId::from(location.to_string()).into(),
        task_resource: None,
        retry_after: Some(Duration::from_secs(retry_after_secs)),
    }
}

pub fn assert_task<T>(response: ModificationResponse<T>, location: &str, retry_after_secs: u64) {
    let ModificationResponse::Task(task) = response else {
        panic!("expected an asynchronous task response");
    };

    assert_eq!(task.location.0.to_string(), location);
    assert!(task.task_resource.is_none());

    assert_eq!(
        task.retry_after,
        Some(Duration::from_secs(retry_after_secs))
    );
}

pub fn assert_empty<T>(response: ModificationResponse<T>) {
    assert!(matches!(response, ModificationResponse::Empty));
}

/// Build a ServiceRoot payload for AMI Viking (`Vendor=AMI`, `RedfishVersion=1.11.0`)
/// merged with the provided `fields`.
pub fn ami_viking_service_root(root_id: &ODataId, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "RootService",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": true
            }
        },
        "Vendor": "AMI",
        "RedfishVersion": "1.11.0",
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{root_id}/SessionService/Sessions"),
            }
        },
    });
    json_merge([&base, &fields])
}

/// Build an AMI ServiceRoot payload (`Vendor=AMI`) with the given
/// `RedfishVersion` and optional AMI OEM `RtpVersion`, merged with `fields`.
///
/// `RtpVersion=Some("13.09.1")` identifies a Grace-based NVIDIA GB300 host BMC.
pub fn ami_service_root(
    root_id: &ODataId,
    redfish_version: &str,
    rtp_version: Option<&str>,
    fields: Value,
) -> Value {
    let mut base = json!({
        ODATA_ID: root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "RootService",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": true
            }
        },
        "Vendor": "AMI",
        "RedfishVersion": redfish_version,
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{root_id}/SessionService/Sessions"),
            }
        },
    });
    if let Some(rtp) = rtp_version {
        base["Oem"] = json!({ "Ami": { "RtpVersion": rtp } });
    }
    json_merge([&base, &fields])
}

/// Build a ServiceRoot payload for Lite-On power shelves merged with `fields`.
///
/// Live shelves may omit the `Systems` navigation property on `/redfish/v1` while
/// `/redfish/v1/Systems` still responds to GET.
pub fn liteon_powershelf_service_root(root_id: &ODataId, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: root_id,
        ODATA_TYPE: "#ServiceRoot.v1_13_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root Service",
        "Vendor": "LITE-ON TECHNOLOGY CORP.",
        "RedfishVersion": "1.17.0",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": false
            }
        },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{root_id}/SessionService/Sessions"),
            }
        },
    });
    json_merge([&base, &fields])
}

/// Build a ServiceRoot payload for anonymous Redfish 1.9.0 platforms merged with
/// the provided `fields`.
pub fn anonymous_1_9_service_root(root_id: &ODataId, fields: Value) -> Value {
    let base = json!({
        ODATA_ID: root_id,
        ODATA_TYPE: "#ServiceRoot.v1_11_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root Service",
        "RedfishVersion": "1.9.0",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": false
            }
        },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{root_id}/SessionService/Sessions"),
            }
        },
    });
    json_merge([&base, &fields])
}

/// Build a Redfish `Actions` payload containing one action target.
pub fn redfish_action_payload(action: &str, target: &str) -> Value {
    let mut action_body = serde_json::Map::new();
    action_body.insert("target".into(), json!(target));

    let mut actions = serde_json::Map::new();
    actions.insert(format!("#{action}"), Value::Object(action_body));

    let mut payload = serde_json::Map::new();
    payload.insert("Actions".into(), Value::Object(actions));
    Value::Object(payload)
}

/// Build a Redfish payload with an empty `Actions` object.
pub fn redfish_empty_actions_payload() -> Value {
    json!({ "Actions": {} })
}

/// Expect a Redfish reset action request with an empty action response.
pub fn expect_redfish_reset_action(bmc: &Bmc, target: &str, reset_type: Option<&str>) {
    let request = reset_type.map_or_else(|| json!({}), |value| json!({ "ResetType": value }));
    bmc.expect(Expect::action(target, request, json!(null)));
}
