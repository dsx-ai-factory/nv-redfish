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
//! Integration tests for AMI ServiceRoot OEM extension support.

use nv_redfish::ServiceRoot;
use nv_redfish_core::ODataId;
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

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_17_0.ServiceRoot";
const AMI_SERVICE_ROOT_DATA_TYPE: &str = "#AMIServiceRoot.v1_0_0.AMIServiceRoot";

#[test]
async fn service_root_ami_rtp_version_parsed() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(
        bmc.clone(),
        root_payload(Some(json!({
            ODATA_TYPE: AMI_SERVICE_ROOT_DATA_TYPE,
            "RtpVersion": "13.09.1",
        }))),
    )
    .await?;

    let ami = root
        .oem_ami_service_root()?
        .expect("AMI ServiceRoot OEM extension should be present");
    assert_eq!(ami.rtp_version(), Some("13.09.1"));

    Ok(())
}

#[test]
async fn service_root_ami_undeclared_oem_properties_retained() -> Result<(), Box<dyn StdError>> {
    // `AMIServiceRoot` declares no `OData.AdditionalProperties` of its
    // own and derives from `Resource.OemObject`, which permits them, so
    // properties the schema does not name must still be readable. The
    // optimizer folds that sole-child base away, and dropping its
    // permission while doing so would silently discard them here.
    let bmc = Arc::new(Bmc::default());
    let root = get_root(
        bmc.clone(),
        root_payload(Some(json!({
            ODATA_TYPE: AMI_SERVICE_ROOT_DATA_TYPE,
            "RtpVersion": "13.09.1",
            "UndeclaredBySchema": "kept",
        }))),
    )
    .await?;

    let ami = root
        .oem_ami_service_root()?
        .expect("AMI ServiceRoot OEM extension should be present");
    assert_eq!(ami.rtp_version(), Some("13.09.1"));
    assert_eq!(
        ami.raw()
            .additional_properties
            .get("UndeclaredBySchema")
            .and_then(serde_json::Value::as_str),
        Some("kept"),
    );

    Ok(())
}

#[test]
async fn service_root_without_ami_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(bmc.clone(), root_payload(None)).await?;

    assert!(root.oem_ami_service_root()?.is_none());

    Ok(())
}

#[test]
async fn service_root_with_null_ami_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let root = get_root(bmc.clone(), root_payload(Some(json!(null)))).await?;

    assert!(root.oem_ami_service_root()?.is_none());

    Ok(())
}

#[test]
async fn service_root_ami_malformed_oem_returns_parse_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(
        bmc.clone(),
        root_payload(Some(json!({
            ODATA_TYPE: AMI_SERVICE_ROOT_DATA_TYPE,
            // Must be a string, but integer is provided deliberately.
            "RtpVersion": 13091
        }))),
    )
    .await?;

    let err = match root.oem_ami_service_root() {
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

async fn get_root(bmc: Arc<Bmc>, payload: Value) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(ODataId::service_root(), payload));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

fn root_payload(ami_oem: Option<Value>) -> Value {
    let root_id = ODataId::service_root();
    let base = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
        "Id": "RootService",
        "Name": "Root Service",
        "Vendor": "AMI",
        "Product": "AMI Redfish Server",
        "RedfishVersion": "1.21.1",
        "ProtocolFeaturesSupported": {
            "ExpandQuery": {
                "NoLinks": true
            }
        },
        "Links": {
            "Sessions": {
                ODATA_ID: format!("{root_id}/SessionService/Sessions"),
            }
        },
    });
    let oem = ami_oem
        .map(|ami| {
            json!({
                "Oem": {
                    "Ami": ami
                }
            })
        })
        .unwrap_or_else(|| json!({}));
    json_merge([&base, &oem])
}
