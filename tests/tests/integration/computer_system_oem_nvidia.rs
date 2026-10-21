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
//! Integration tests for NVIDIA ComputerSystem OEM support.

use nv_redfish::computer_system::ComputerSystem;
use nv_redfish::oem::nvidia::computer_system::HostRshim;
use nv_redfish::oem::nvidia::computer_system::Mode;
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
const SYSTEM_COLLECTION_DATA_TYPE: &str = "#ComputerSystemCollection.ComputerSystemCollection";
const SYSTEM_DATA_TYPE: &str = "#ComputerSystem.v1_19_0.ComputerSystem";
const NVIDIA_SYSTEM_DATA_TYPE: &str = "#NvidiaComputerSystem.v1_0_0.NvidiaComputerSystem";

/// Service-root identity of an NVIDIA DPU.
const DPU_VENDOR: &str = "Nvidia";
const DPU_PRODUCT: &str = "Nvidia-BMCMezz";
const BF3_DPU_PRODUCT: &str = "BlueField-3 DPU";

#[test]
async fn oem_nvidia_dpu_missing_odata_id_in_oem_target_payload() -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVIDIA BlueField DPU.
    // Quirk under test: missing @odata.id in OEM target payload.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                "Nvidia": { ODATA_ID: &ids.nvidia_oem_id }
            })),
        ),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.nvidia_oem_id,
        json!({
            ODATA_TYPE: NVIDIA_SYSTEM_DATA_TYPE,
            "BaseMAC": "1070fd010203",
            "Mode": "NicMode",
        }),
    ));

    let oem = system
        .oem_nvidia()
        .await?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(
        oem.base_mac().map(|v| v.to_string()),
        Some("1070fd010203".into())
    );
    assert_eq!(oem.mode(), Some(Mode::NicMode));

    Ok(())
}

#[test]
async fn oem_nvidia_dpu_with_odata_id_still_supported() -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVIDIA BlueField DPU.
    // Regression check: regular payload with @odata.id remains supported.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                "Nvidia": { ODATA_ID: &ids.nvidia_oem_id }
            })),
        ),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.nvidia_oem_id,
        json!({
            ODATA_ID: &ids.nvidia_oem_id,
            ODATA_TYPE: NVIDIA_SYSTEM_DATA_TYPE,
            "BaseMAC": "aabbccddeeff",
            "Mode": "DpuMode",
        }),
    ));

    let oem = system
        .oem_nvidia()
        .await?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(
        oem.base_mac().map(|v| v.to_string()),
        Some("aabbccddeeff".into())
    );
    assert_eq!(oem.mode(), Some(Mode::DpuMode));

    Ok(())
}

#[test]
async fn oem_nvidia_bluefield3_product_fetches_oem_target() -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVIDIA BlueField-3 DPU using its product name.
    // Quirk under test: the BF3 product identity selects NVIDIA DPU handling.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system_on_platform(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                "Nvidia": { ODATA_ID: &ids.nvidia_oem_id }
            })),
        ),
        DPU_VENDOR,
        BF3_DPU_PRODUCT,
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.nvidia_oem_id,
        json!({
            ODATA_ID: &ids.nvidia_oem_id,
            ODATA_TYPE: NVIDIA_SYSTEM_DATA_TYPE,
            "BaseMAC": "aabbccddeeff",
            "Mode": "NicMode",
        }),
    ));

    let oem = system
        .oem_nvidia()
        .await?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(
        oem.base_mac().map(|v| v.to_string()),
        Some("aabbccddeeff".into())
    );
    assert_eq!(oem.mode(), Some(Mode::NicMode));

    Ok(())
}

#[test]
async fn system_without_nvidia_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // Platform under test: generic system without NVIDIA OEM payload.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(bmc.clone(), &ids, system_payload(&ids, None)).await?;

    assert!(system.oem_nvidia().await?.is_none());

    Ok(())
}

#[test]
async fn system_with_null_nvidia_oem_returns_none() -> Result<(), Box<dyn StdError>> {
    // Some firmware spells "no extension" as an explicit null rather
    // than by omitting the key. It must read as absence, not as a
    // parse failure.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(&ids, Some(json!({ "Nvidia": Value::Null }))),
    )
    .await?;

    assert!(system.oem_nvidia().await?.is_none());

    Ok(())
}

#[test]
async fn oem_nvidia_dpu_inline_oem_object_shape_supported() -> Result<(), Box<dyn StdError>> {
    // Platform under test: NVIDIA BlueField DPU.
    // Regression check: inline Oem.Nvidia object shape in ComputerSystem response.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                "Nvidia": {
                    ODATA_ID: &ids.nvidia_oem_id,
                    ODATA_TYPE: "#NvidiaComputerSystem.v1_3_0.NvidiaComputerSystem",
                    "SystemConfigProfile": {
                        ODATA_ID: format!("{}/SystemConfigProfile", ids.nvidia_oem_id),
                        ODATA_TYPE: "#SystemConfigProfile.v1_0_0.SystemConfigProfile"
                    }
                }
            })),
        ),
    )
    .await?;

    // Inline Oem.Nvidia shape still resolves via @odata.id fetch path.
    bmc.expect(Expect::get(
        &ids.nvidia_oem_id,
        json!({
            ODATA_ID: &ids.nvidia_oem_id,
            ODATA_TYPE: "#NvidiaComputerSystem.v1_3_0.NvidiaComputerSystem",
            "BaseMAC": "001122334455",
            "Mode": "NicMode"
        }),
    ));
    let oem = system
        .oem_nvidia()
        .await?
        .expect("NVIDIA OEM extension must be available");
    assert_eq!(
        oem.base_mac().map(|v| v.to_string()),
        Some("001122334455".into())
    );
    assert_eq!(oem.mode(), Some(Mode::NicMode));

    Ok(())
}

#[test]
async fn oem_nvidia_inline_properties_are_available_on_non_dpu_platform(
) -> Result<(), Box<dyn StdError>> {
    // A non-DPU platform exposes the reported schema properties without
    // the extra GET required by the DPU's partially expanded OEM object.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system_on_platform(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                "Nvidia": {
                    ODATA_ID: &ids.nvidia_oem_id,
                    ODATA_TYPE: NVIDIA_SYSTEM_DATA_TYPE,
                    "ISTModeEnabled": true,
                    "BaseMAC": "1070fd010203",
                    "Mode": "DpuMode",
                    "HostRshim": "Enabled",
                }
            })),
        ),
        "NVIDIA",
        "Some Other Product",
    )
    .await?;

    // No `Expect` is registered for `nvidia_oem_id`: reaching out to it
    // would fail the mock, proving the OEM resource is never fetched.
    let oem = system
        .oem_nvidia()
        .await?
        .expect("NVIDIA OEM extension must be available");

    assert_eq!(oem.raw().ist_mode_enabled.flatten(), Some(true));
    assert_eq!(
        oem.base_mac().map(|mac| mac.to_string()),
        Some("1070fd010203".into())
    );
    assert_eq!(oem.mode(), Some(Mode::DpuMode));
    assert_eq!(oem.host_rshim(), Some(HostRshim::Enabled));

    Ok(())
}

#[test]
async fn oem_nvidia_dpu_exposes_standard_and_local_schema_properties(
) -> Result<(), Box<dyn StdError>> {
    // The fetched resource exposes both the published NVIDIA properties
    // and the local BlueField schema additions on the same handle.
    let bmc = Arc::new(Bmc::default());
    let ids = ids();
    let system = get_system(
        bmc.clone(),
        &ids,
        system_payload(
            &ids,
            Some(json!({
                "Nvidia": { ODATA_ID: &ids.nvidia_oem_id }
            })),
        ),
    )
    .await?;

    bmc.expect(Expect::get(
        &ids.nvidia_oem_id,
        json!({
            ODATA_ID: &ids.nvidia_oem_id,
            ODATA_TYPE: "#NvidiaComputerSystem.v1_0_0.NvidiaComputerSystem",
            "ISTModeEnabled": true,
            "BaseMAC": "1070fd010203",
            "Mode": "DpuMode",
        }),
    ));

    let oem = system
        .oem_nvidia()
        .await?
        .expect("NVIDIA OEM extension must be available");

    // Declared by the schema.
    assert_eq!(oem.raw().ist_mode_enabled.flatten(), Some(true));
    // Undeclared, surfaced by the quirk.
    assert_eq!(
        oem.base_mac().map(|v| v.to_string()),
        Some("1070fd010203".into())
    );
    assert_eq!(oem.mode(), Some(Mode::DpuMode));

    Ok(())
}

async fn get_system(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    get_system_on_platform(bmc, ids, member, DPU_VENDOR, DPU_PRODUCT).await
}

async fn get_system_on_platform(
    bmc: Arc<Bmc>,
    ids: &Ids,
    member: Value,
    vendor: &str,
    product: &str,
) -> Result<ComputerSystem<Bmc>, Box<dyn StdError>> {
    let root = expect_service_root(bmc.clone(), ids, vendor, product).await?;
    bmc.expect(Expect::expand(
        &ids.systems_id,
        json!({
            ODATA_ID: &ids.systems_id,
            ODATA_TYPE: SYSTEM_COLLECTION_DATA_TYPE,
            "Id": "Systems",
            "Name": "Computer System Collection",
            "Members": [member]
        }),
    ));

    let systems = root.systems().await?.unwrap();
    let members = systems.members().await?;
    assert_eq!(members.len(), 1);
    Ok(members
        .into_iter()
        .next()
        .expect("single system must exist"))
}

async fn expect_service_root(
    bmc: Arc<Bmc>,
    ids: &Ids,
    vendor: &str,
    product: &str,
) -> Result<ServiceRoot<Bmc>, Box<dyn StdError>> {
    bmc.expect(Expect::get(
        &ids.root_id,
        json!({
            ODATA_ID: &ids.root_id,
            ODATA_TYPE: SERVICE_ROOT_DATA_TYPE,
            "Id": "RootService",
            "Name": "RootService",
            // Platform identity: only the NVIDIA DPU unlocks reading
            // the undeclared `BaseMAC` / `Mode` properties.
            "Vendor": vendor,
            "Product": product,
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "Systems": { ODATA_ID: &ids.systems_id },
            "Links": {
                "Sessions": {
                    ODATA_ID: format!("{}/SessionService/Sessions", ids.root_id),
                }
            },
        }),
    ));
    ServiceRoot::new(bmc).await.map_err(Into::into)
}

struct Ids {
    root_id: ODataId,
    systems_id: String,
    system_id: String,
    nvidia_oem_id: String,
}

fn ids() -> Ids {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/Bluefield");
    let nvidia_oem_id = format!("{system_id}/Oem/Nvidia");
    Ids {
        root_id,
        systems_id,
        system_id,
        nvidia_oem_id,
    }
}

fn system_payload(ids: &Ids, nvidia_oem: Option<Value>) -> Value {
    let base = json!({
        ODATA_ID: &ids.system_id,
        ODATA_TYPE: SYSTEM_DATA_TYPE,
        "Id": "Bluefield",
        "Name": "Bluefield",
        "Status": {
            "Health": "OK",
            "State": "Enabled"
        }
    });
    let oem = nvidia_oem.map_or_else(
        || json!({}),
        |nvidia| {
            json!({
                "Oem": nvidia
            })
        },
    );
    json_merge([&base, &oem])
}
