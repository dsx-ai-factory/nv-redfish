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

//! Integration tests for AMI UpdateService request composition.

use futures_util::io::AsyncReadExt as _;
use nv_redfish::oem::ami::update_service::oem_parameters_part;
use nv_redfish::oem::ami::update_service::AmiUpdateServiceUpdate;
use nv_redfish::oem::ami::update_service::AmiUpdateServiceUpdateExt as _;
use nv_redfish::oem::ami::update_service::CpldFirmwareType;
use nv_redfish::oem::ami::update_service::ImageType;
use nv_redfish::oem::ami::update_service::OemParametersUpdate;
use nv_redfish::oem::ami::update_service::PreserveConfigurationUpdate;
use nv_redfish::schema::resource::OemUpdate;
use nv_redfish::update_service::UpdateServiceUpdate;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use serde_json::json;
use std::error::Error as StdError;
use std::sync::Arc;

const UPDATE_SERVICE_ID: &str = "/redfish/v1/UpdateService";

#[tokio::test]
async fn ami_preserve_configuration_composes_with_standard_update() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    bmc.expect(Expect::get(&ODataId::service_root(), service_root()));
    bmc.expect(Expect::get(UPDATE_SERVICE_ID, update_service()));
    let root = ServiceRoot::new(bmc.clone()).await?;
    let service = root
        .update_service()
        .await?
        .ok_or("UpdateService missing")?;
    let update = UpdateServiceUpdate::builder()
        .with_oem(OemUpdate {
            additional_properties: json!({ "OtherVendor": { "Keep": true } }),
        })
        .build()
        .with_oem_ami(
            AmiUpdateServiceUpdate::builder()
                .with_preserve_configuration(preserve_configuration())
                .build(),
        )?;
    let request = json!({
        "Oem": {
            "OtherVendor": { "Keep": true },
            "AMIUpdateService": {
                "PreserveConfiguration": {
                    "Authentication": true,
                    "EXTLOG": true,
                    "FRU": true,
                    "IPMI": true,
                    "KVM": true,
                    "NTP": true,
                    "Network": true,
                    "REDFISH": true,
                    "SDR": true,
                    "SEL": true,
                    "SNMP": true,
                    "SSH": true,
                    "Syslog": true,
                    "WEB": true
                }
            }
        }
    });
    bmc.expect(Expect::update(
        UPDATE_SERVICE_ID,
        &request,
        update_service(),
    ));

    assert!(matches!(
        service.update(&update).await?,
        ModificationResponse::Entity(_)
    ));
    Ok(())
}

#[tokio::test]
async fn ami_oem_parameters_create_exact_multipart_json() -> Result<(), Box<dyn StdError>> {
    let bmc = OemParametersUpdate::builder()
        .with_image_type(ImageType::Bmc)
        .build();
    assert_eq!(
        read_part(oem_parameters_part(&bmc)?).await?,
        json!({ "ImageType": "BMC" })
    );

    let bios = OemParametersUpdate::builder()
        .with_image_type(ImageType::Bios)
        .with_preserve_bios("true".into())
        .build();
    assert_eq!(
        read_part(oem_parameters_part(&bios)?).await?,
        json!({ "ImageType": "BIOS", "PreserveBIOS": "true" })
    );

    let psu = OemParametersUpdate::builder()
        .with_image_type(ImageType::Psu)
        .build();
    assert_eq!(
        read_part(oem_parameters_part(&psu)?).await?,
        json!({ "ImageType": "PSU" })
    );

    let cpld = OemParametersUpdate::builder()
        .with_image_type(ImageType::Fpga)
        .with_cpldfw_type(CpldFirmwareType::Mb)
        .build();
    assert_eq!(
        read_part(oem_parameters_part(&cpld)?).await?,
        json!({ "ImageType": "FPGA", "CPLDFWType": "MB" })
    );
    Ok(())
}

fn preserve_configuration() -> PreserveConfigurationUpdate {
    PreserveConfigurationUpdate::builder()
        .with_authentication(true)
        .with_extlog(true)
        .with_fru(true)
        .with_ipmi(true)
        .with_kvm(true)
        .with_ntp(true)
        .with_network(true)
        .with_redfish(true)
        .with_sdr(true)
        .with_sel(true)
        .with_snmp(true)
        .with_ssh(true)
        .with_syslog(true)
        .with_web(true)
        .build()
}

async fn read_part(
    mut part: nv_redfish_core::OemMultipartPart,
) -> Result<serde_json::Value, Box<dyn StdError>> {
    assert_eq!(part.name, "OemParameters");
    assert_eq!(part.content_type.as_deref(), Some("application/json"));
    let mut bytes = Vec::new();
    part.reader.read_to_end(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn service_root() -> serde_json::Value {
    json!({
        ODATA_ID: "/redfish/v1",
        "Id": "RootService",
        "Name": "Root Service",
        "Links": {
            "Sessions": { ODATA_ID: "/redfish/v1/SessionService/Sessions" }
        },
        "UpdateService": { ODATA_ID: UPDATE_SERVICE_ID }
    })
}

fn update_service() -> serde_json::Value {
    json!({
        ODATA_ID: UPDATE_SERVICE_ID,
        "Id": "UpdateService",
        "Name": "Update Service"
    })
}
