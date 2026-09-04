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

use nv_redfish_bmc_http::reqwest::BmcError;
use nv_redfish_bmc_http::reqwest::Client;
use nv_redfish_bmc_http::reqwest::ClientParams;
use nv_redfish_bmc_http::BmcCredentials;
use nv_redfish_bmc_http::CacheSettings;
use nv_redfish_bmc_http::HttpBmc;
use nv_redfish_core::query::ExpandQuery;
use nv_redfish_core::Creatable;
use nv_redfish_core::Deletable;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use redfish_std::redfish::manager_account::ManagerAccount;
use redfish_std::redfish::manager_account::ManagerAccountCreate;
use redfish_std::redfish::service_root::ServiceRoot;
use url::Url;

#[tokio::main]
async fn main() -> Result<(), BmcError> {
    let client = Client::with_params(ClientParams::new()).map_err(BmcError::ReqwestError)?;

    let creds = BmcCredentials::new("username".into(), "password".into());
    let bmc = HttpBmc::new(
        client,
        Url::parse("https://192.168.2.2").unwrap(),
        creds,
        CacheSettings::default(),
    );

    let service_root = NavProperty::<ServiceRoot>::new_reference(ODataId::service_root())
        .get(&bmc)
        .await?;

    let chassis_members = &service_root
        .chassis
        .as_ref()
        .unwrap()
        .get(&bmc)
        .await?
        .members;

    let chassis = chassis_members.iter().next().unwrap().get(&bmc).await?;

    let all_devices = &chassis
        .pcie_devices
        .as_ref()
        .unwrap()
        .get(&bmc)
        .await?
        .members;

    for device in all_devices {
        if let Some(nav_prop) = &device.get(&bmc).await?.pcie_functions {
            let function_handles = nav_prop.get(&bmc).await?;
            for function_handle in &function_handles.members {
                let _function = function_handle.get(&bmc).await?;
            }

            println!("{function_handles:?}"); // unpolulated members
            let function_handles = function_handles
                .expand(&bmc, ExpandQuery::default())
                .await?;
            println!("{function_handles:?}"); // members populated
        }
    }

    let system = &service_root
        .systems
        .as_ref()
        .expect("no systems")
        .get(&bmc)
        .await?
        .members
        .first()
        .expect("at least one system")
        .get(&bmc)
        .await?;

    println!("{system:?}");

    let ac = &service_root
        .account_service
        .as_ref()
        .expect("no account service")
        .get(&bmc)
        .await?;
    println!("{ac:?}");

    // Should use cache here
    let _ = &service_root
        .account_service
        .as_ref()
        .expect("no account service")
        .get(&bmc)
        .await?;

    let _change_password = system
        .bios
        .as_ref()
        .expect("no bios")
        .get(&bmc)
        .await?
        .actions
        .as_ref()
        .expect("no actions")
        .change_password
        .as_ref()
        .expect("no reset action")
        .run(
            &bmc,
            &redfish_std::redfish::bios::BiosChangePasswordAction {
                password_name: "admin".into(),
                old_password: Some("admin1".into()),
                new_password: "admin2".into(),
            },
        )
        .await?;

    // Crud operations
    let account = ac
        .accounts
        .as_ref()
        .expect("no accounts")
        .create(
            &bmc,
            &ManagerAccountCreate::builder(
                "secret_password".into(),
                "Administrator".into(),
                "admin".into(),
            )
            .build(),
        )
        .await?;
    println!("{account:?}");
    let account = match account {
        ModificationResponse::Entity(account) => account,
        other => panic!("Unexpected create outcome: {:?}", other),
    };

    let acc = NavProperty::<ManagerAccount>::new_reference(account.odata_id().clone())
        .get(&bmc)
        .await?;

    let _delete = acc.delete(&bmc).await?;

    let _ = NavProperty::<ManagerAccount>::new_reference(account.odata_id().clone())
        .get(&bmc)
        .await?;

    Ok(())
}
