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
//! Integration tests for Manager DellAttributes OEM extension.

use nv_redfish::manager::Manager;
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
const MANAGER_COLLECTION_DATA_TYPE: &str = "#ManagerCollection.ManagerCollection";
const MANAGER_DATA_TYPE: &str = "#Manager.v1_18_0.Manager";
const DELL_ATTRS_DATA_TYPE: &str = "#DellAttributes.v1_0_0.DellAttributes";

#[test]
async fn manager_dell_attributes_lean_payload() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = manager_ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload(&ids, true)).await?;

    bmc.expect(Expect::expand(
        &ids.dell_attrs_id,
        dell_attributes_payload(&ids),
    ));

    let attrs = manager.oem_dell_attributes().await?.unwrap();
    assert!(attrs
        .attribute("CurrentNIC.1.MTU")
        .is_some_and(|v| v.integer_value() == Some(1500)));
    assert!(attrs
        .attribute("CurrentNIC.1.Hostname")
        .is_some_and(|v| v.str_value().is_some_and(|s| s == "idrac-embedded")));
    assert!(attrs
        .attribute("CurrentNIC.1.ProxyEnabled")
        .is_some_and(|v| v.bool_value() == Some(true)));
    assert!(attrs
        .attribute("CurrentNIC.1.OptionalNull")
        .is_some_and(|v| v.is_null()));
    assert!(attrs.attribute("CurrentNIC.1.Unknown").is_none());

    Ok(())
}

#[test]
async fn manager_without_dell_oem_returns_not_available() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = manager_ids();
    let manager = get_manager(bmc.clone(), &ids, manager_payload(&ids, false)).await?;

    assert!(manager.oem_dell_attributes().await?.is_none());

    Ok(())
}

#[test]
async fn manager_with_null_dell_oem_returns_not_available() -> Result<(), Box<dyn StdError>> {
    // An explicit null under the vendor key means "no extension";
    // it must read as absence, not as a parse failure.
    //
    // No GET for the crafted DellAttributes URL is expected: the null
    // must gate the fetch, not just the return value.
    let bmc = Arc::new(Bmc::default());
    let ids = manager_ids();
    let manager = get_manager(
        bmc.clone(),
        &ids,
        json_merge([
            &manager_payload(&ids, false),
            &json!({ "Oem": { "Dell": null } }),
        ]),
    )
    .await?;

    assert!(manager.oem_dell_attributes().await?.is_none());

    Ok(())
}

async fn get_manager(
    bmc: Arc<Bmc>,
    ids: &ManagerIds,
    manager: Value,
) -> Result<Manager<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids).await?;
    bmc.expect(Expect::expand(
        &ids.manager_collection_id,
        json!({
            ODATA_ID: &ids.manager_collection_id,
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
    ids: &ManagerIds,
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
            "Managers": { ODATA_ID: &ids.manager_collection_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));

    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct ManagerIds {
    root_id: ODataId,
    manager_collection_id: String,
    manager_id: String,
    dell_attrs_id: String,
}

fn manager_ids() -> ManagerIds {
    let root_id = ODataId::service_root();
    let manager_collection_id = format!("{root_id}/Managers");
    let manager_id = format!("{manager_collection_id}/iDRAC.Embedded.1");
    let dell_attrs_id = format!("{manager_id}/Oem/Dell/DellAttributes/iDRAC.Embedded.1");
    ManagerIds {
        root_id,
        manager_collection_id,
        manager_id,
        dell_attrs_id,
    }
}

fn manager_payload(ids: &ManagerIds, with_dell_oem: bool) -> Value {
    let base = json!({
        ODATA_ID: &ids.manager_id,
        ODATA_TYPE: MANAGER_DATA_TYPE,
        "Id": "iDRAC.Embedded.1",
        "Name": "iDRAC.Embedded.1",
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    });
    let oem = if with_dell_oem {
        json!({
            "Oem": {
                "Dell": {}
            }
        })
    } else {
        json!({})
    };
    json_merge([&base, &oem])
}

fn dell_attributes_payload(ids: &ManagerIds) -> Value {
    json!({
        ODATA_ID: &ids.dell_attrs_id,
        ODATA_TYPE: DELL_ATTRS_DATA_TYPE,
        "AttributeRegistry": "ManagerAttributeRegistry.v1_0_0",
        "Attributes": {
            "CurrentNIC.1.MTU": 1500,
            "CurrentNIC.1.ProxyEnabled": true,
            "CurrentNIC.1.Hostname": "idrac-embedded",
            "CurrentNIC.1.OptionalNull": null
        },
        "Description": "This schema provides the oem attributes",
        "Id": "iDRAC.Embedded.1",
        "Name": "OEMAttributeRegistry"
    })
}
