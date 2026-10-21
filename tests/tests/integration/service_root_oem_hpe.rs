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
//! Integration tests for HPE ServiceRoot OEM extension support.

use nv_redfish::oem::hpe::ilo_service_ext::ManagerType;
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

const SERVICE_ROOT_DATA_TYPE: &str = "#ServiceRoot.v1_13_0.ServiceRoot";
const HPE_SERVICE_EXT_DATA_TYPE: &str = "#HpeiLOServiceExt.v2_5_0.HpeiLOServiceExt";

#[test]
async fn service_root_hpe_ilo_manager_type_parsed() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(
        bmc.clone(),
        root_payload(Some(json!({
            ODATA_TYPE: HPE_SERVICE_EXT_DATA_TYPE,
            "Manager": [
                {
                    "ManagerType": "iLO 6"
                }
            ]
        }))),
    )
    .await?;

    let hpe = root
        .oem_hpe_ilo_service_ext()?
        .expect("HPE ServiceRoot OEM extension should be present");
    assert!(matches!(hpe.manager_type(), Some(ManagerType::Ilo(6))));

    Ok(())
}

#[test]
async fn service_root_hpe_manager_type_other_variant() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(
        bmc.clone(),
        root_payload(Some(json!({
            ODATA_TYPE: HPE_SERVICE_EXT_DATA_TYPE,
            "Manager": [
                {
                    "ManagerType": "Custom"
                }
            ]
        }))),
    )
    .await?;

    let hpe = root
        .oem_hpe_ilo_service_ext()?
        .expect("HPE ServiceRoot OEM extension should be present");
    assert!(matches!(
        hpe.manager_type(),
        Some(ManagerType::Other("Custom"))
    ));

    Ok(())
}

#[test]
async fn service_root_without_hpe_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(bmc.clone(), root_payload(None)).await?;

    assert!(root.oem_hpe_ilo_service_ext()?.is_none());

    Ok(())
}

#[test]
async fn service_root_with_null_hpe_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    let bmc = Arc::new(Bmc::default());
    let root = get_root(bmc.clone(), root_payload(Some(json!(null)))).await?;

    assert!(root.oem_hpe_ilo_service_ext()?.is_none());

    Ok(())
}

#[test]
async fn service_root_hpe_malformed_oem_returns_parse_error() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root = get_root(
        bmc.clone(),
        root_payload(Some(json!({
            ODATA_TYPE: HPE_SERVICE_EXT_DATA_TYPE,
            // Must be a collection, but here object is provided deliberately.
            "Manager": {
                "ManagerType": "iLO 6"
            }
        }))),
    )
    .await?;

    let err = match root.oem_hpe_ilo_service_ext() {
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

fn root_payload(hpe_oem: Option<Value>) -> Value {
    let root_id = ODataId::service_root();
    let base = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
        "Id": "RootService",
        "Name": "RootService",
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
    let oem = hpe_oem
        .map(|hpe| {
            json!({
                "Oem": {
                    "Hpe": hpe
                }
            })
        })
        .unwrap_or_else(|| json!({}));
    json_merge([&base, &oem])
}
