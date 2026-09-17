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

//! Integration tests of Update Service.

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

use futures_util::io::Cursor;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish::schema::update_service::HttpPushUriOptionsUpdate;
use nv_redfish::update_service::MultipartUpdateParameters;
use nv_redfish::update_service::UpdateService;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish::update_service::UpdateServiceUpdate;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use nv_redfish_core::DataStream;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::MultipartUpdateRequest;
use nv_redfish_core::ODataId;
use nv_redfish_core::OemMultipartPart;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::UploadStream;
use nv_redfish_tests::ami_viking_service_root;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use tokio::test;

const UPDATE_SERVICE_DATA_TYPE: &str = "#UpdateService.v1_9_0.UpdateService";
const SW_INVENTORIES_DATA_TYPE: &str = "#SoftwareInventoryCollection.SoftwareInventoryCollection";
const SW_INVENTORY_DATA_TYPE: &str = "#SoftwareInventory.v1_4_0.SoftwareInventory";

const UPDATE_SERVICE_URI: &str = "/redfish/v1/UpdateService";
#[cfg(feature = "update-service-deprecated")]
const HTTP_PUSH_URI: &str = "/redfish/v1/UpdateService/update";
const MULTIPART_URI: &str = "/redfish/v1/UpdateService/update-multipart";

#[cfg(feature = "update-service-deprecated")]
#[derive(serde::Deserialize)]
struct UploadResponse {
    #[serde(rename = "Result")]
    result: String,
}

#[test]
async fn list_dell_fw_inventores() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let update_service = get_update_service(bmc.clone(), &root_id, "Dell").await?;
    let update_service_raw = update_service.raw();
    let update_service_id = update_service_raw.odata_id();
    let fw_inventories_id = format!("{update_service_id}/FirmwareInventory");
    let fw_inventory_id =
        format!("{fw_inventories_id}/Installed-0-2.1.3__Disk.Bay.0:Enclosure.Internal.0-1");
    let fw_inventory_id2 =
        format!("{fw_inventories_id}/Installed-0-3.0.0__Disk.Bay.1:Enclosure.Internal.0-1");
    bmc.expect(Expect::expand(
        &fw_inventories_id,
        json!({
            ODATA_ID: &fw_inventory_id,
            ODATA_TYPE: &SW_INVENTORIES_DATA_TYPE,
            "Name": "Firmware Inventory Collection",
            "Members": [
                {
                    "@odata.id": &fw_inventory_id,
                    "@odata.type": &SW_INVENTORY_DATA_TYPE,
                    "Id": "Installed-0-1.0.0__Disk.Bay.0:Enclosure.Internal.0-1",
                    "Name": "PCIe SSD in Slot 0 in Bay 1",
                    "ReleaseDate": "00:00:00Z",
                    "SoftwareId": "0",
                    "Status": {
                        "Health": "OK",
                        "State": "Enabled"
                    },
                    "Updateable": true,
                    "Version": "1.0.0"
                },
                {
                    "@odata.id": &fw_inventory_id2,
                    "@odata.type": &SW_INVENTORY_DATA_TYPE,
                    "Id": "Installed-0-3.0.0__Disk.Bay.1:Enclosure.Internal.0-1",
                    "Name": "PCIe SSD in Slot 0 in Bay 2",
                    "ReleaseDate": "0000-00-00T00:00:00Z",
                    "SoftwareId": "0",
                    "Status": {
                        "Health": "OK",
                        "State": "Enabled"
                    },
                    "Updateable": true,
                    "Version": "3.0.0"
                }
            ]
        }),
    ));
    let inventories = update_service.firmware_inventories().await?.unwrap();
    assert_eq!(inventories.len(), 2);
    assert!(inventories[0].raw().release_date.is_none());
    assert!(inventories[1].raw().release_date.is_none());
    Ok(())
}

#[test]
async fn ami_viking_missing_root_update_service_nav_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let update_service_id = format!("{root_id}/UpdateService");
    let fw_inventory_id = format!("{update_service_id}/FirmwareInventory");

    bmc.expect(Expect::get(
        &root_id,
        ami_viking_service_root(&root_id, json!({})),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &update_service_id,
        json!({
            ODATA_ID: &update_service_id,
            ODATA_TYPE: &UPDATE_SERVICE_DATA_TYPE,
            "Id": "UpdateService",
            "Name": "UpdateService",
            "FirmwareInventory": {
                ODATA_ID: &fw_inventory_id,
            },
        }),
    ));

    let update_service = service_root.update_service().await?;
    assert!(update_service.is_some());

    Ok(())
}

#[test]
async fn ami_viking_missing_update_service_name_workaround() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    let update_service_id = format!("{root_id}/UpdateService");

    bmc.expect(Expect::get(
        &root_id,
        ami_viking_service_root(
            &root_id,
            json!({
                "UpdateService": {
                    ODATA_ID: &update_service_id,
                }
            }),
        ),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        &update_service_id,
        json!({
            ODATA_ID: &update_service_id,
            ODATA_TYPE: &UPDATE_SERVICE_DATA_TYPE,
            "Id": "UpdateService",
        }),
    ));

    let update_service = service_root.update_service().await?.unwrap();
    assert_eq!(update_service.raw().name, "Unnamed update service");

    Ok(())
}

async fn get_update_service(
    bmc: Arc<Bmc>,
    root_id: &ODataId,
    vendor: &str,
) -> Result<UpdateService<Bmc>, Box<dyn StdError>> {
    let update_service_id = format!("{root_id}/UpdateService");
    let data_type = "#ServiceRoot.v1_13_0.ServiceRoot";
    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: &data_type,
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "UpdateService": {
                ODATA_ID: &update_service_id,
            },
            "Vendor": vendor,
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{root_id}/SessionService/Sessions"),
                }
            },
        }),
    ));
    let service_root = ServiceRoot::new(bmc.clone()).await?;

    let fw_inventory_id = format!("{update_service_id}/FirmwareInventory");
    bmc.expect(Expect::get(
        &update_service_id,
        json!({
            ODATA_ID: &update_service_id,
            ODATA_TYPE: &UPDATE_SERVICE_DATA_TYPE,
            "Id": "UpdateService",
            "Name": "UpdateService",
            "FirmwareInventory": {
                ODATA_ID: &fw_inventory_id,
            },
        }),
    ));
    Ok(service_root.update_service().await?.unwrap())
}

#[tokio::test]
async fn uses_multipart_http_push_uri() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());

    bmc.expect(Expect::get("/redfish/v1", service_root_json()));
    bmc.expect(Expect::get(
        UPDATE_SERVICE_URI,
        update_service_json(Some(MULTIPART_URI)),
    ));

    bmc.expect(Expect::multipart_update(
        MULTIPART_URI,
        json!({
            "ForceUpdate": true,
            "Targets": ["/redfish/v1/Systems/1"]
        }),
        "firmware.bin",
        json!({
            "@odata.id": "/redfish/v1/TaskService/Tasks/42",
            "Id": "42"
        }),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("expected update service")?;
    let parameters = MultipartUpdateParameters::builder()
        .with_force_update(true)
        .with_targets(vec!["/redfish/v1/Systems/1".to_string()])
        .build();

    let response = update_service
        .multipart_update_from_reader::<_, _, serde_json::Value>(
            &parameters,
            DataStream::new("firmware.bin", Cursor::new(b"firmware".to_vec()))
                .with_content_length(8),
            Duration::from_secs(600),
        )
        .await?;

    let ModificationResponse::Entity(body) = response else {
        return Err(String::from("expected entity response").into());
    };

    assert_eq!(body["@odata.id"], "/redfish/v1/TaskService/Tasks/42");

    Ok(())
}

#[cfg(feature = "update-service-deprecated")]
#[tokio::test]
async fn uses_http_push_uri_without_update_parameters() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());

    bmc.expect(Expect::get("/redfish/v1", service_root_json()));
    bmc.expect(Expect::get(
        UPDATE_SERVICE_URI,
        update_service_json_with_uris(None, Some(HTTP_PUSH_URI)),
    ));

    bmc.expect(Expect::http_push_uri_update(
        HTTP_PUSH_URI,
        json!({
            "Result": "accepted"
        }),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("expected update service")?;

    let response = update_service
        .http_push_uri_update_from_reader::<_, UploadResponse>(
            UploadStream::new(Cursor::new(b"firmware".to_vec())).with_content_length(8),
            Duration::from_secs(600),
        )
        .await?;

    let ModificationResponse::Entity(body) = response else {
        return Err(String::from("expected entity response").into());
    };

    assert_eq!(body.result, "accepted");

    Ok(())
}

#[cfg(feature = "update-service-deprecated")]
#[tokio::test]
async fn patches_http_push_uri_options_and_targets() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());

    bmc.expect(Expect::get("/redfish/v1", service_root_json()));
    bmc.expect(Expect::get(
        UPDATE_SERVICE_URI,
        update_service_json_with_uris(None, Some(HTTP_PUSH_URI)),
    ));

    let update = UpdateServiceUpdate::builder()
        .with_http_push_uri_targets(vec!["/redfish/v1/Systems/1".to_string()])
        .with_http_push_uri_targets_busy(true)
        .with_http_push_uri_options(
            HttpPushUriOptionsUpdate::builder()
                .with_force_update(true)
                .build(),
        )
        .with_http_push_uri_options_busy(true)
        .build();

    bmc.expect(Expect::update(
        UPDATE_SERVICE_URI,
        json!({
            "HttpPushUriTargets": ["/redfish/v1/Systems/1"],
            "HttpPushUriTargetsBusy": true,
            "HttpPushUriOptions": {
                "ForceUpdate": true
            },
            "HttpPushUriOptionsBusy": true
        }),
        update_service_json_with_uris(None, Some(HTTP_PUSH_URI)),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("expected update service")?;

    let response = update_service.update(&update).await?;

    let ModificationResponse::Entity(updated) = response else {
        return Err(String::from("expected entity response").into());
    };

    assert_eq!(updated.raw().http_push_uri.as_deref(), Some(HTTP_PUSH_URI));

    Ok(())
}

#[tokio::test]
async fn uses_generated_update_parameters_with_oem_parts() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());

    bmc.expect(Expect::get("/redfish/v1", service_root_json()));
    bmc.expect(Expect::get(
        UPDATE_SERVICE_URI,
        update_service_json(Some(MULTIPART_URI)),
    ));

    let oem = nv_redfish::schema::resource::OemUpdate {
        additional_properties: json!({
            "Nvidia": {
                "Mode": "Rms"
            }
        }),
    };
    let parameters = MultipartUpdateParameters::builder()
        .with_force_update(true)
        .with_targets(vec!["/redfish/v1/Systems/1".to_string()])
        .with_oem(oem)
        .build();

    let expected_parameters = serde_json::to_value(&parameters)?;

    bmc.expect(Expect::multipart_update_with_oem_parts(
        MULTIPART_URI,
        expected_parameters,
        "firmware.bin",
        ["OemNvidia"],
        json!({
            "@odata.id": "/redfish/v1/TaskService/Tasks/77",
            "Id": "77"
        }),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("expected update service")?;

    let request = MultipartUpdateRequest {
        update_parameters: &parameters,
        update_stream: DataStream::new("firmware.bin", Cursor::new(b"firmware".to_vec())),
        oem_parts: vec![OemMultipartPart::new(
            "OemNvidia",
            Cursor::new(br#"{"Mode":"Rms"}"#.to_vec()),
        )?
        .with_content_type("application/json")],
        upload_timeout: Duration::from_secs(600),
    };

    let response = update_service
        .multipart_update::<_, _, serde_json::Value>(request)
        .await?;

    let ModificationResponse::Entity(body) = response else {
        return Err(String::from("expected entity response").into());
    };

    assert_eq!(body["@odata.id"], "/redfish/v1/TaskService/Tasks/77");

    Ok(())
}

#[tokio::test]
async fn requires_multipart_http_push_uri() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());

    bmc.expect(Expect::get("/redfish/v1", service_root_json()));

    // Multipart uploads must use the service-provided endpoint, so a
    // missing MultipartHttpPushUri should fail before any upload is sent.
    bmc.expect(Expect::get(UPDATE_SERVICE_URI, update_service_json(None)));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("expected update service")?;

    let result = update_service
        .multipart_update_from_reader::<_, _, serde_json::Value>(
            &MultipartUpdateParameters::default(),
            DataStream::new("firmware.bin", Cursor::new(b"firmware".to_vec()))
                .with_content_length(8),
            Duration::from_secs(600),
        )
        .await;

    assert!(matches!(
        result,
        Err(Error::UpdateServiceMultipartHttpPushUriNotAvailable)
    ));

    Ok(())
}

#[cfg(feature = "update-service-deprecated")]
#[tokio::test]
async fn requires_http_push_uri() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());

    bmc.expect(Expect::get("/redfish/v1", service_root_json()));

    // Raw uploads must use the service-provided endpoint, so a missing
    // HttpPushUri should fail before any upload is sent.
    bmc.expect(Expect::get(UPDATE_SERVICE_URI, update_service_json(None)));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("expected update service")?;

    let result = update_service
        .http_push_uri_update_from_reader::<_, UploadResponse>(
            UploadStream::new(Cursor::new(b"firmware".to_vec())).with_content_length(8),
            Duration::from_secs(600),
        )
        .await;

    assert!(matches!(
        result,
        Err(Error::UpdateServiceHttpPushUriNotAvailable)
    ));

    Ok(())
}

fn service_root_json() -> serde_json::Value {
    json!({
        "@odata.id": "/redfish/v1",
        "Id": "RootService",
        "Name": "Root Service",
        "Links": {
            "Sessions": {
                "@odata.id": "/redfish/v1/SessionService/Sessions"
            }
        },
        "UpdateService": {
            "@odata.id": UPDATE_SERVICE_URI
        }
    })
}

fn update_service_json(multipart_uri: Option<&str>) -> serde_json::Value {
    update_service_json_with_uris(multipart_uri, None)
}

fn update_service_json_with_uris(
    multipart_uri: Option<&str>,
    http_push_uri: Option<&str>,
) -> serde_json::Value {
    let mut body = json!({
        "@odata.id": UPDATE_SERVICE_URI,
        "Id": "UpdateService",
        "Name": "Update Service"
    });

    if let Some(multipart_uri) = multipart_uri {
        body["MultipartHttpPushUri"] = json!(multipart_uri);
    }

    if let Some(http_push_uri) = http_push_uri {
        body["HttpPushUri"] = json!(http_push_uri);
    }

    body
}
