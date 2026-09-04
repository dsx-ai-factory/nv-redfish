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
use nv_redfish::oem::nvidia::computer_system::Mode;
use nv_redfish::resource::ResetType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::redfish_action_payload;
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
const CHASSIS_COLLECTION_DATA_TYPE: &str = "#ChassisCollection.ChassisCollection";
const CHASSIS_DATA_TYPE: &str = "#Chassis.v1_22_0.Chassis";

/// Service-root identity of an NVIDIA DPU.
const DPU_VENDOR: &str = "Nvidia";
const DPU_PRODUCT: &str = "Nvidia-BMCMezz";
const BF3_DPU_PRODUCT: &str = "BlueField-3 DPU";

#[test]
async fn force_restart_prefers_advertised_nvidia_dpu_reset() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bluefield_4_ids();
    let standard_target = format!("{}/Actions/ComputerSystem.Reset", ids.system_id);
    let base = system_payload(&ids, None);
    let standard_action = redfish_action_payload("ComputerSystem.Reset", &standard_target);
    let system = get_system(bmc.clone(), &ids, json_merge([&base, &standard_action])).await?;
    let oem_target = expect_bluefield_chassis(&bmc, &ids, true);
    bmc.expect(Expect::action(
        &oem_target,
        json!({ "ResetType": "ForceDpuReset" }),
        json!(null),
    ));

    assert!(matches!(
        system.reset(Some(ResetType::ForceRestart)).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn force_restart_falls_back_when_nvidia_dpu_reset_is_absent() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let ids = bluefield_4_ids();
    let standard_target = format!("{}/Actions/ComputerSystem.Reset", ids.system_id);
    let base = system_payload(&ids, None);
    let standard_action = redfish_action_payload("ComputerSystem.Reset", &standard_target);
    let system = get_system(bmc.clone(), &ids, json_merge([&base, &standard_action])).await?;
    expect_bluefield_chassis(&bmc, &ids, false);
    bmc.expect(Expect::action(
        &standard_target,
        json!({ "ResetType": "ForceRestart" }),
        json!(null),
    ));

    assert!(matches!(
        system.reset(Some(ResetType::ForceRestart)).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn force_restart_falls_back_when_nvidia_dpu_reset_target_is_stale(
) -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let ids = bluefield_4_ids();
    let standard_target = format!("{}/Actions/ComputerSystem.Reset", ids.system_id);
    let base = system_payload(&ids, None);
    let standard_action = redfish_action_payload("ComputerSystem.Reset", &standard_target);
    let system = get_system(bmc.clone(), &ids, json_merge([&base, &standard_action])).await?;
    let oem_target = expect_bluefield_chassis(&bmc, &ids, true);
    bmc.expect(Expect::action_not_found(
        &oem_target,
        json!({ "ResetType": "ForceDpuReset" }),
    ));
    bmc.expect(Expect::action(
        &standard_target,
        json!({ "ResetType": "ForceRestart" }),
        json!(null),
    ));

    assert!(matches!(
        system.reset(Some(ResetType::ForceRestart)).await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

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
async fn oem_nvidia_quirks_ignored_on_non_dpu_platform() -> Result<(), Box<dyn StdError>> {
    // Both DPU quirks -- fetching the OEM object out of line and
    // reading the undeclared `BaseMAC` / `Mode` from it -- apply only
    // on the platform known to need them. A non-DPU BMC serving the
    // same properties is parsed as plain schema: the extension is still
    // available, the undeclared properties are not, and nothing is
    // fetched.
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
    assert!(oem.base_mac().is_none());
    assert!(oem.mode().is_none());

    Ok(())
}

#[test]
async fn oem_nvidia_dpu_exposes_schema_and_quirk_together() -> Result<(), Box<dyn StdError>> {
    // The resource deserializes as the NVIDIA `NvidiaComputerSystem`
    // schema type, while `BaseMAC` / `Mode` -- which that schema marks
    // `AdditionalProperties=false` and cannot carry -- stay reachable
    // as a platform quirk on the same handle.
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
            "Chassis": { ODATA_ID: format!("{}/Chassis", ids.root_id) },
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

fn bluefield_4_ids() -> Ids {
    let root_id = ODataId::service_root();
    let systems_id = format!("{root_id}/Systems");
    let system_id = format!("{systems_id}/BlueField_0");
    let nvidia_oem_id = format!("{system_id}/Oem/Nvidia");
    Ids {
        root_id,
        systems_id,
        system_id,
        nvidia_oem_id,
    }
}

fn expect_bluefield_chassis(bmc: &Bmc, ids: &Ids, with_reset_action: bool) -> String {
    let chassis_collection_id = format!("{}/Chassis", ids.root_id);
    let chassis_id = format!("{chassis_collection_id}/BlueField_0");
    let action_target = format!("{chassis_id}/Actions/Oem/NvidiaChassis.Reset");
    bmc.expect(Expect::get(
        &chassis_collection_id,
        json!({
            ODATA_ID: &chassis_collection_id,
            ODATA_TYPE: CHASSIS_COLLECTION_DATA_TYPE,
            "Id": "Chassis",
            "Name": "Chassis Collection",
            "Members": [{ ODATA_ID: &chassis_id }]
        }),
    ));
    let actions = if with_reset_action {
        json!({
            "Actions": {
                "Oem": {
                    "#NvidiaChassis.Reset": {
                        "target": &action_target
                    }
                }
            }
        })
    } else {
        json!({ "Actions": { "Oem": {} } })
    };
    let chassis = json!({
        ODATA_ID: &chassis_id,
        ODATA_TYPE: CHASSIS_DATA_TYPE,
        "Id": "BlueField_0",
        "Name": "BlueField_0",
        "ChassisType": "Card"
    });
    bmc.expect(Expect::get(&chassis_id, json_merge([&chassis, &actions])));

    action_target
}

fn system_payload(ids: &Ids, nvidia_oem: Option<Value>) -> Value {
    let name = ids
        .system_id
        .rsplit('/')
        .next()
        .unwrap_or(ids.system_id.as_str());
    let base = json!({
        ODATA_ID: &ids.system_id,
        ODATA_TYPE: SYSTEM_DATA_TYPE,
        "Id": name,
        "Name": name,
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
