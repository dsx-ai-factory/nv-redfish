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

//! Inherited fields in generated read and update models.

use nv_redfish::schema::computer_system::ComputerSystem;
use nv_redfish::schema::manager_account::ManagerAccountUpdate;
use nv_redfish::schema::resource::OemUpdate;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::RedfishSettings;
use nv_redfish_tests::base::redfish::inheritance::{Child, ChildUpdate};
use serde_json::json;

#[test]
fn resource_reads_inherited_fields_and_settings() {
    let resource: ComputerSystem = serde_json::from_value(json!({
        "@odata.id": "/redfish/v1/Systems/1",
        "@odata.etag": "revision-1",
        "@odata.type": "#ComputerSystem.v1_20_0.ComputerSystem",
        "Id": "1",
        "Name": "Server",
        "Description": null,
        "Oem": { "Vendor": { "Custom": 42 } },
        "@Redfish.Settings": {
            "SettingsObject": { "@odata.id": "/redfish/v1/Systems/1/Settings" }
        }
    }))
    .expect("resource deserializes");

    assert_eq!(resource.id, "1");
    assert_eq!(resource.name, "Server");
    assert_eq!(resource.description, Some(None));
    assert_eq!(resource.etag(), resource.odata_etag.as_ref());
    assert_eq!(
        resource
            .oem
            .as_ref()
            .expect("OEM fields")
            .additional_properties,
        json!({ "Vendor": { "Custom": 42 } }),
    );
    assert_eq!(
        resource
            .settings_object()
            .expect("settings link")
            .id()
            .to_string(),
        "/redfish/v1/Systems/1/Settings",
    );
}

#[test]
fn update_serializes_inherited_fields_and_redacts_secrets() {
    let update = ManagerAccountUpdate::builder()
        .with_oem(OemUpdate {
            additional_properties: json!({ "Vendor": { "Secret": "oem-secret" } }),
        })
        .with_password("password-secret".into())
        .build();
    assert_eq!(
        serde_json::to_value(&update).expect("update serializes"),
        json!({
            "Oem": { "Vendor": { "Secret": "oem-secret" } },
            "Password": "password-secret",
        })
    );
    assert_eq!(
        serde_json::to_value(ManagerAccountUpdate::builder().build()).expect("empty update"),
        json!({}),
    );
    let debug = format!("{update:?}");
    assert!(!debug.contains("oem-secret"));
    assert!(!debug.contains("password-secret"));
}

#[test]
fn complex_types_read_and_update_inherited_fields() {
    let child: Child = serde_json::from_value(json!({
        "Label": "inherited",
        "Enabled": true,
        "Secret": "read-secret",
        "Value": 42,
    }))
    .expect("derived complex type deserializes");
    assert_eq!(child.label.as_deref(), Some("inherited"));
    assert_eq!(child.enabled, Some(true));
    assert_eq!(child.value, Some(42));
    assert!(!format!("{child:?}").contains("read-secret"));

    let update = ChildUpdate::builder()
        .with_enabled(false)
        .with_secret("update-secret".into())
        .with_value(43)
        .build();
    assert!(!format!("{update:?}").contains("update-secret"));
    assert_eq!(
        serde_json::to_value(update).expect("derived update serializes"),
        json!({
            "Enabled": false,
            "Secret": "update-secret",
            "Value": 43,
        })
    );
}
