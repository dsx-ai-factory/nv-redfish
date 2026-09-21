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

//! Integration tests for standard ComponentIntegrity and SPDM APIs.

use std::error::Error as StdError;
use std::sync::Arc;

use nv_redfish::certificate::CertificateType;
use nv_redfish::component_integrity::ComponentIntegrity;
use nv_redfish::component_integrity::ComponentIntegrityType;
use nv_redfish::core::AsyncTask;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;

const COLLECTION_ID: &str = "/redfish/v1/ComponentIntegrity";
const COMPONENT_ID: &str = "/redfish/v1/ComponentIntegrity/HGX_IRoT_GPU_0";
const CERTIFICATE_ID: &str = "/redfish/v1/Chassis/HGX_IRoT_GPU_0/Certificates/CertChain";
const VIKING_COMPONENT_ID: &str = "/redfish/v1/ComponentIntegrity/EROT_BIOS_0";
const VIKING_CERTIFICATE_ID: &str = "/redfish/v1/Chassis/EROT_BIOS_0/Certificates/CertChain";
const NVIDIA_ACTION_TARGET: &str = "/redfish/v1/ComponentIntegrity/HGX_IRoT_GPU_0/Actions/ComponentIntegrity.SPDMGetSignedMeasurements";
const VIKING_ACTION_TARGET: &str =
    "/redfish/v1/ComponentIntegrity/EROT_BIOS_0/Actions/SPDMGetSignedMeasurements";

#[tokio::test]
async fn component_integrity_follows_advertised_links_and_certificate(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component(bmc.clone(), NVIDIA_ACTION_TARGET).await?;

    assert_eq!(component.raw().id, "HGX_IRoT_GPU_0");
    assert_eq!(
        component.raw().component_integrity_type,
        ComponentIntegrityType::Spdm
    );
    assert_eq!(component.raw().component_integrity_enabled, Some(true));
    assert_eq!(
        component
            .spdm_get_signed_measurements_target()
            .map(ToString::to_string)
            .as_deref(),
        Some(NVIDIA_ACTION_TARGET)
    );

    bmc.expect(Expect::get(
        CERTIFICATE_ID,
        json!({
            ODATA_ID: CERTIFICATE_ID,
            ODATA_TYPE: "#Certificate.v1_5_0.Certificate",
            "CertificateString": "-----BEGIN CERTIFICATE-----\ncertificate\n-----END CERTIFICATE-----\n",
            "CertificateType": "PEMchain",
            "CertificateUsageTypes": ["Device"],
            "Id": "CertChain",
            "Name": "HGX_IRoT_GPU_0 Certificate Chain",
            "SPDM": {
                "SlotId": 0
            }
        }),
    ));

    let certificate = component
        .component_certificate()
        .await?
        .expect("SPDM certificate is advertised");
    let certificate = certificate.raw();
    assert_eq!(
        certificate.certificate_type,
        Some(Some(CertificateType::PeMchain))
    );
    assert_eq!(
        certificate
            .spdm
            .as_ref()
            .and_then(|spdm| spdm.slot_id.as_ref())
            .and_then(Option::as_ref),
        Some(&0)
    );

    Ok(())
}

#[tokio::test]
async fn viking_certificate_normalizes_pem_chain_spelling() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component(bmc.clone(), VIKING_ACTION_TARGET).await?;

    bmc.expect(Expect::get(
        VIKING_CERTIFICATE_ID,
        json!({
            ODATA_ID: VIKING_CERTIFICATE_ID,
            ODATA_TYPE: "#CertificateCollection.CertificateCollection",
            "CertificateString": "-----BEGIN CERTIFICATE-----\ncertificate\n-----END CERTIFICATE-----\n",
            "CertificateType": "PEMChain",
            "Id": "CertChain",
            "Name": "EROT Certificate Chain",
            "SPDM": {
                "SlotId": 0
            }
        }),
    ));

    let certificate = component
        .component_certificate()
        .await?
        .expect("SPDM certificate is advertised")
        .raw();
    assert_eq!(
        certificate.certificate_type,
        Some(Some(CertificateType::PeMchain))
    );
    assert!(certificate.certificate_usage_types.is_none());
    assert_eq!(component.raw().component_integrity_type_version, "unknown");

    Ok(())
}

#[tokio::test]
async fn spdm_signed_measurements_return_synchronous_evidence() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component(bmc.clone(), NVIDIA_ACTION_TARGET).await?;
    let nonce = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    bmc.expect(Expect::action(
        NVIDIA_ACTION_TARGET,
        json!({
            "Nonce": nonce,
            "SlotId": 0,
            "MeasurementIndices": [255]
        }),
        json!({
            "HashingAlgorithm": "TPM_ALG_SHA_512",
            "SignedMeasurements": "signed-evidence",
            "SigningAlgorithm": "TPM_ALG_ECDSA_ECC_NIST_P384",
            "Version": "1.1.0"
        }),
    ));

    let ModificationResponse::Entity(evidence) = component
        .spdm_get_signed_measurements(Some(nonce.to_string()), Some(0), Some(vec![255]))
        .await?
    else {
        return Err("expected synchronous signed measurements".into());
    };
    assert_eq!(evidence.signed_measurements, "signed-evidence");
    assert_eq!(evidence.version, "1.1.0");

    Ok(())
}

#[tokio::test]
async fn viking_signed_measurements_use_advertised_target_and_preserve_task(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component(bmc.clone(), VIKING_ACTION_TARGET).await?;
    let nonce = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let task_id = "/redfish/v1/TaskService/Tasks/89";

    bmc.expect(Expect::action_task(
        VIKING_ACTION_TARGET,
        json!({ "Nonce": nonce }),
        AsyncTask {
            location: ODataId::from(task_id.to_string()).into(),
            task_resource: Some(ODataId::from(task_id.to_string())),
            retry_after: None,
        },
    ));

    let ModificationResponse::Task(task) = component
        .spdm_get_signed_measurements(Some(nonce.to_string()), None, None)
        .await?
    else {
        return Err("expected asynchronous signed measurements".into());
    };
    assert_eq!(task.location.0.to_string(), task_id);
    assert_eq!(task.task_status_uri().to_string(), task_id);

    Ok(())
}

#[tokio::test]
async fn spdm_signed_measurements_fetch_nvidia_data_endpoint() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component(bmc.clone(), NVIDIA_ACTION_TARGET).await?;
    let data_id = format!("{NVIDIA_ACTION_TARGET}/data");

    bmc.expect(Expect::get(
        &data_id,
        json!({
            "HashingAlgorithm": "TPM_ALG_SHA_512",
            "SignedMeasurements": "signed-evidence",
            "SigningAlgorithm": "TPM_ALG_ECDSA_ECC_NIST_P384",
            "Version": "1.1.0"
        }),
    ));

    let evidence = component.spdm_signed_measurements_data().await?;
    assert_eq!(evidence.raw().signed_measurements, "signed-evidence");
    assert_eq!(evidence.raw().version, "1.1.0");

    Ok(())
}

#[tokio::test]
async fn spdm_signed_measurements_preserve_empty_response() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component(bmc.clone(), NVIDIA_ACTION_TARGET).await?;

    bmc.expect(Expect::action_empty(NVIDIA_ACTION_TARGET, json!({})));

    assert!(matches!(
        component
            .spdm_get_signed_measurements(None, None, None)
            .await?,
        ModificationResponse::Empty
    ));

    Ok(())
}

#[tokio::test]
async fn component_integrity_returns_none_when_not_advertised() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let root_id = ODataId::service_root();
    bmc.expect(Expect::get(&root_id, service_root(&root_id, false, false)));

    let root = ServiceRoot::new(bmc).await?;
    assert!(root.component_integrity().await?.is_none());

    Ok(())
}

#[tokio::test]
async fn spdm_signed_measurements_require_advertised_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let component = component_without_actions(bmc).await?;

    assert!(matches!(
        component
            .spdm_get_signed_measurements(None, None, None)
            .await,
        Err(Error::ActionNotAvailable)
    ));

    Ok(())
}

async fn component(
    bmc: Arc<Bmc>,
    action_target: &str,
) -> Result<ComponentIntegrity<Bmc>, Box<dyn StdError>> {
    let viking = action_target == VIKING_ACTION_TARGET;
    let component_id = if viking {
        VIKING_COMPONENT_ID
    } else {
        COMPONENT_ID
    };
    let certificate_id = if viking {
        VIKING_CERTIFICATE_ID
    } else {
        CERTIFICATE_ID
    };
    let action_info = format!("{component_id}/SPDMGetSignedMeasurementsActionInfo");
    let action = if viking {
        json!({
            "@redfish.ActionInfo": action_info,
            "target": action_target
        })
    } else {
        json!({
            "@Redfish.ActionInfo": action_info,
            "target": action_target
        })
    };
    component_with_actions(
        bmc,
        json!({
            "#ComponentIntegrity.SPDMGetSignedMeasurements": action
        }),
        component_id,
        certificate_id,
        viking,
    )
    .await
}

async fn component_without_actions(
    bmc: Arc<Bmc>,
) -> Result<ComponentIntegrity<Bmc>, Box<dyn StdError>> {
    component_with_actions(bmc, json!({}), COMPONENT_ID, CERTIFICATE_ID, false).await
}

async fn component_with_actions(
    bmc: Arc<Bmc>,
    actions: Value,
    component_id: &str,
    certificate_id: &str,
    viking: bool,
) -> Result<ComponentIntegrity<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    bmc.expect(Expect::get(&root_id, service_root(&root_id, true, viking)));
    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::get(
        COLLECTION_ID,
        json!({
            ODATA_ID: COLLECTION_ID,
            ODATA_TYPE: "#ComponentIntegrityCollection.ComponentIntegrityCollection",
            "Name": "Component Integrity Collection",
            "Members": [{ ODATA_ID: component_id }],
            "Members@odata.count": 1
        }),
    ));
    let collection = root
        .component_integrity()
        .await?
        .expect("ComponentIntegrity is advertised");

    bmc.expect(Expect::get(
        component_id,
        json!({
            ODATA_ID: component_id,
            ODATA_TYPE: "#ComponentIntegrity.v1_0_0.ComponentIntegrity",
            "Actions": actions,
            "ComponentIntegrityEnabled": true,
            "ComponentIntegrityType": "SPDM",
            "ComponentIntegrityTypeVersion": if viking { "unknown" } else { "1.1.0" },
            "Id": if viking { "EROT_BIOS_0" } else { "HGX_IRoT_GPU_0" },
            "Links": {
                "ComponentsProtected": [{
                    ODATA_ID: "/redfish/v1/Systems/HGX_Baseboard_0/Processors/GPU_0"
                }]
            },
            "Name": if viking {
                "SPDM Integrity for EROT_BIOS_0"
            } else {
                "SPDM Integrity for HGX_IRoT_GPU_0"
            },
            "SPDM": {
                "IdentityAuthentication": {
                    "ResponderAuthentication": {
                        "ComponentCertificate": {
                            ODATA_ID: certificate_id
                        }
                    }
                },
                "Requester": {
                    ODATA_ID: "/redfish/v1/Managers/HGX_BMC_0"
                }
            },
            "TargetComponentURI": if viking {
                "/redfish/v1/Chassis/EROT_BIOS_0"
            } else {
                "/redfish/v1/Chassis/HGX_IRoT_GPU_0"
            }
        }),
    ));

    collection
        .members()
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| "expected one ComponentIntegrity member".into())
}

fn service_root(root_id: &ODataId, include_component_integrity: bool, viking: bool) -> Value {
    let mut root = json!({
        ODATA_ID: root_id,
        ODATA_TYPE: "#ServiceRoot.v1_19_0.ServiceRoot",
        "Id": "RootService",
        "Name": "Root service",
        "Links": {
            "Sessions": { ODATA_ID: "/redfish/v1/SessionService/Sessions" }
        }
    });
    if include_component_integrity {
        root["ComponentIntegrity"] = json!({ ODATA_ID: COLLECTION_ID });
    }
    if viking {
        root[ODATA_TYPE] = json!("#ServiceRoot.v1_13_0.ServiceRoot");
        root["Vendor"] = json!("AMI");
        root["Product"] = json!("AMI Redfish Server");
        root["RedfishVersion"] = json!("1.11.0");
    }
    root
}
