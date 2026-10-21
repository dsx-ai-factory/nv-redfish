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

//! Serialization of create requests generated from the standard Redfish schemas.

use nv_redfish::schema::resource::{Health, OemCreate, StatusCreate};
use nv_redfish::schema::volume::{LinksCreate, RaidType, VolumeCreate};
use nv_redfish_core::{Reference, ReferenceLeaf};
use serde_json::json;

#[test]
fn volume_create_omits_properties_not_required_on_create() {
    assert_eq!(
        serde_json::to_value(VolumeCreate::builder().build()).expect("serializable"),
        json!({}),
    );
}

#[test]
fn volume_create_serializes_inherited_properties_and_drive_references() {
    let drive = Reference::from(&ReferenceLeaf {
        odata_id: "/redfish/v1/Systems/1/Storage/1/Drives/1"
            .to_string()
            .into(),
    });
    let create = VolumeCreate::builder()
        .with_name("Boot".into())
        .with_raid_type(RaidType::Raid1)
        .with_links(LinksCreate::builder().with_drives(vec![drive]).build())
        .build();

    assert_eq!(
        serde_json::to_value(create).expect("serializable"),
        json!({
            "Name": "Boot",
            "RAIDType": "RAID1",
            "Links": {
                "Drives": [{ "@odata.id": "/redfish/v1/Systems/1/Storage/1/Drives/1" }],
            },
        }),
    );
}

#[test]
fn volume_create_supports_read_only_complex_values_and_oem_properties() {
    let mut oem = OemCreate::builder().build();
    oem.additional_properties = json!({ "Vendor": { "Secret": "oem-secret-sentinel" } });
    let create = VolumeCreate::builder()
        .with_status(StatusCreate::builder().with_health(Health::Ok).build())
        .with_oem(oem)
        .build();

    assert_eq!(
        serde_json::to_value(&create).expect("serializable"),
        json!({
            "Status": { "Health": "OK" },
            "Oem": { "Vendor": { "Secret": "oem-secret-sentinel" } },
        }),
    );
    assert!(!format!("{create:?}").contains("oem-secret-sentinel"));
}
