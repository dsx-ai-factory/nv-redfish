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

mod fixtures;

use std::convert::Infallible;
use std::error::Error;

use nv_redfish_bmc_mock::Bmc;
use nv_redfish_core::{
    Bmc as _, EntityTypeRef, ModificationResponse, NavProperty, ODataId, Updatable,
};
use redfish_oem_contoso::redfish::contoso_turboencabulator_service::{
    ContosoTurboencabulatorServiceUpdate, TurboencabulatorMode,
};
use redfish_std::redfish::manager_account::{ManagerAccount, ManagerAccountCreate};
use redfish_std::redfish::resource::ResetType;
use redfish_std::redfish::service_root::ServiceRoot;

use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize, Debug)]
struct Contoso {
    #[serde(rename = "Contoso")]
    oem_root: redfish_oem_contoso::redfish::contoso_service_root::ServiceRoot,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let bmc = fixtures::scripted_bmc();

    let service_root = NavProperty::<ServiceRoot>::new_reference(ODataId::service_root())
        .get(&bmc)
        .await?;

    discover_chassis(&bmc, &service_root).await?;

    let systems = &service_root
        .systems
        .as_ref()
        .ok_or("field is absent: systems")?
        .get(&bmc)
        .await?;

    let system = systems
        .members
        .first()
        .ok_or("field is absent: first system")?
        .get(&bmc)
        .await?;

    println!("System {} (id: {}):", system.name, system.id);

    println!(
        "  BIOS Version: {}",
        system
            .bios_version
            .as_ref()
            .and_then(Option::as_ref)
            .unwrap_or(&"unknown".to_string())
    );

    println!("Performing system reset...");

    let _reset = system
        .actions
        .as_ref()
        .ok_or("field is absent: actions")?
        .reset(&bmc, Some(ResetType::ForceRestart))
        .await?;

    println!("  Ok!");
    println!("Browse OEM extension:");

    let contoso_oem: Contoso = serde_json::from_value(
        service_root
            .oem
            .as_ref()
            .ok_or("field is absent: oem")?
            .additional_properties
            .clone(),
    )
    .map_err(|error| format!("cannot fill OEM: {error}"))?;

    let turboencabulator_service = contoso_oem
        .oem_root
        .turboencabulator_service
        .ok_or("field is absent: turboencabulator_service")?
        .get(&bmc)
        .await?;

    println!("  Turboencabulator service:");

    println!(
        "    service enabled: {}",
        turboencabulator_service
            .service_enabled
            .as_ref()
            .and_then(Option::as_ref)
            .map(ToString::to_string)
            .unwrap_or("unknown".to_string())
    );

    println!(
        "    will government buy: {}",
        turboencabulator_service
            .will_government_buy
            .as_ref()
            .and_then(Option::as_ref)
            .map(ToString::to_string)
            .unwrap_or("unknown".to_string())
    );

    println!(
        "    mode: {:?}",
        turboencabulator_service.turboencabulator_mode
    );

    let update = ContosoTurboencabulatorServiceUpdate::builder()
        .with_turboencabulator_mode(TurboencabulatorMode::Turbo);

    println!(
        "BMC Update {}: {}",
        turboencabulator_service.odata_id(),
        serde_json::to_string(&update)?
    );

    let updated = turboencabulator_service.update(&bmc, &update).await?;

    let ModificationResponse::Entity(updated) = updated else {
        return Err("field is absent: typed update payload".into());
    };

    updated.refresh(&bmc).await?;

    create_account(&bmc, &service_root).await?;

    println!("Read mock SSE stream:");

    let mut event_stream = bmc.stream::<Value>("/redfish/v1/EventService/SSE").await?;

    while let Some(event) = event_stream.next().await {
        let event = event?;
        println!("  {:?}", event);
    }

    Ok(())
}

async fn discover_chassis(
    bmc: &Bmc<Infallible>,
    service_root: &ServiceRoot,
) -> Result<(), Box<dyn Error>> {
    let chassis = &service_root
        .chassis
        .as_ref()
        .ok_or("field is absent: chassis")?
        .get(bmc)
        .await?;

    println!("Discoveried chassis:");

    for member in &chassis.members {
        println!("  {}", member.id());
    }

    for chassis in &chassis.members {
        let chassis = chassis.get(bmc).await?;

        println!("Chassis: {} (id: {})", chassis.name, chassis.id);

        println!(
            "  Model: {}",
            chassis
                .model
                .as_ref()
                .and_then(Option::as_ref)
                .unwrap_or(&"unknown".to_string())
        );

        let pcie_devices = chassis
            .pcie_devices
            .as_ref()
            .ok_or("field is absent: pcie_devices")?
            .get(bmc)
            .await?;

        for pcie_device in &pcie_devices.members {
            let pcie_device = pcie_device.get(bmc).await?;

            println!(
                "  PCI Device: {} (id: {})",
                pcie_device.name, pcie_device.id
            );

            let pcie_functions = pcie_device
                .pcie_functions
                .as_ref()
                .ok_or("field is absent: pcie_functions")?
                .get(bmc)
                .await?;

            for pcie_function in &pcie_functions.members {
                let pcie_function = pcie_function.get(bmc).await?;

                println!(
                    "    Function: {} (id: {})",
                    pcie_function.name, pcie_function.id
                );
            }
        }
    }

    Ok(())
}

async fn create_account(
    bmc: &Bmc<Infallible>,
    service_root: &ServiceRoot,
) -> Result<(), Box<dyn Error>> {
    println!("Create account");

    let accounts = service_root
        .account_service
        .as_ref()
        .ok_or("field is absent: account_service")?
        .get(bmc)
        .await?
        .accounts
        .as_ref()
        .ok_or("field is absent: accounts")?
        .get(bmc)
        .await?;

    let request = ManagerAccountCreate::builder(
        "secret_password".into(),
        "Administrator".into(),
        "admin".into(),
    )
    .build();

    println!(
        "BMC create {}: {}",
        accounts.odata_id(),
        serde_json::to_string(&request)?
    );

    let account = bmc
        .create::<_, ManagerAccount>(accounts.odata_id(), &request)
        .await?;

    println!("  Ok!");
    println!("Returned account:");

    let ModificationResponse::Entity(account) = account else {
        return Err("field is absent: typed account payload".into());
    };

    println!(
        "  User name: {}",
        account.user_name.as_ref().unwrap_or(&"-".to_string())
    );

    println!(
        "  Enabled: {}",
        account
            .enabled
            .map(|b| b.to_string())
            .unwrap_or("-".to_string())
    );

    Ok(())
}
