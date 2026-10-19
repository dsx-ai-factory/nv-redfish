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

//! Integration tests for UEFI Secure Boot databases and certificates.

use std::error::Error as StdError;
use std::sync::Arc;
use std::time::Duration;

use nv_redfish::certificate::CertificateCreate;
use nv_redfish::certificate::CertificateType;
use nv_redfish::computer_system::SecureBoot;
use nv_redfish::computer_system::SecureBootDatabaseResetKeysType;
use nv_redfish::computer_system::SecureBootResetKeysType;
use nv_redfish::ServiceRoot;
use nv_redfish_core::AsyncTask;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::ODataId;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;

const SYSTEMS_ID: &str = "/redfish/v1/Systems";
const SYSTEM_ID: &str = "/redfish/v1/Systems/System_0";
const SECURE_BOOT_ID: &str = "/redfish/v1/Systems/System_0/SecureBoot";
const DATABASES_ID: &str = "/redfish/v1/Systems/System_0/SecureBoot/SecureBootDatabases";
const PK_DATABASE_ID: &str = "/redfish/v1/Systems/System_0/SecureBoot/SecureBootDatabases/PK";
const CERTIFICATES_ID: &str =
    "/redfish/v1/Systems/System_0/SecureBoot/SecureBootDatabases/PK/Certificates";

#[tokio::test]
async fn secure_boot_database_certificates_are_typed_and_creatable() -> Result<(), Box<dyn StdError>>
{
    let bmc = Arc::new(Bmc::default());
    let secure_boot = secure_boot(
        bmc.clone(),
        json!({
            "SecureBootDatabases": { ODATA_ID: DATABASES_ID }
        }),
    )
    .await?;

    bmc.expect(Expect::get(
        DATABASES_ID,
        json!({
            ODATA_ID: DATABASES_ID,
            ODATA_TYPE: "#SecureBootDatabaseCollection.SecureBootDatabaseCollection",
            "Name": "UEFI SecureBoot Database Collection",
            "Members": [{ ODATA_ID: PK_DATABASE_ID }],
            "Members@odata.count": 1
        }),
    ));
    let databases = secure_boot
        .databases()
        .await?
        .expect("SecureBootDatabases is advertised");

    let reset_target = format!("{PK_DATABASE_ID}/Actions/SecureBootDatabase.ResetKeys");
    bmc.expect(Expect::get(
        PK_DATABASE_ID,
        json!({
            ODATA_ID: PK_DATABASE_ID,
            ODATA_TYPE: "#SecureBootDatabase.v1_0_1.SecureBootDatabase",
            "Actions": {
                "#SecureBootDatabase.ResetKeys": {
                    "target": &reset_target
                }
            },
            "Certificates": { ODATA_ID: CERTIFICATES_ID },
            "DatabaseId": "PK",
            "Id": "PK",
            "Name": "PK Database"
        }),
    ));
    let mut databases = databases.members().await?;
    let database = databases.pop().expect("one database is returned");
    assert_eq!(database.raw().database_id.as_deref(), Some("PK"));

    let existing_certificate_id = format!("{CERTIFICATES_ID}/1");
    bmc.expect(Expect::get(
        CERTIFICATES_ID,
        json!({
            ODATA_ID: CERTIFICATES_ID,
            ODATA_TYPE: "#CertificateCollection.CertificateCollection",
            "@Redfish.SupportedCertificates": ["PEM"],
            "Name": "Certificate Collection",
            "Members": [{ ODATA_ID: &existing_certificate_id }],
            "Members@odata.count": 1
        }),
    ));
    let certificates = database
        .certificates()
        .await?
        .expect("Certificates is advertised");

    bmc.expect(Expect::get(
        &existing_certificate_id,
        certificate(&existing_certificate_id, "existing"),
    ));
    let members = certificates.members().await?;
    assert_eq!(members.len(), 1);
    assert_eq!(members[0].raw().id, "existing");

    let create = CertificateCreate::builder("pem-data".to_string(), CertificateType::Pem).build();
    let created_certificate_id = format!("{CERTIFICATES_ID}/2");
    bmc.expect(Expect::create(
        CERTIFICATES_ID,
        json!({
            "CertificateString": "pem-data",
            "CertificateType": "PEM"
        }),
        json!({ ODATA_ID: &created_certificate_id }),
    ));
    bmc.expect(Expect::get(
        &created_certificate_id,
        certificate(&created_certificate_id, "created"),
    ));

    let ModificationResponse::Entity(created) = certificates.create(&create).await? else {
        return Err("expected created certificate".into());
    };
    assert_eq!(created.raw().id, "created");

    bmc.expect(Expect::action(
        &reset_target,
        json!({ "ResetKeysType": "DeleteAllKeys" }),
        json!(null),
    ));
    assert!(matches!(
        database
            .reset_keys(SecureBootDatabaseResetKeysType::DeleteAllKeys)
            .await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[tokio::test]
async fn secure_boot_reset_keys_uses_advertised_action() -> Result<(), Box<dyn StdError>> {
    let bmc = Arc::new(Bmc::default());
    let reset_target = format!("{SECURE_BOOT_ID}/Actions/SecureBoot.ResetKeys");
    let secure_boot = secure_boot(
        bmc.clone(),
        json!({
            "Actions": {
                "#SecureBoot.ResetKeys": {
                    "target": &reset_target
                }
            }
        }),
    )
    .await?;

    bmc.expect(Expect::action(
        &reset_target,
        json!({ "ResetKeysType": "DeletePK" }),
        json!(null),
    ));
    assert!(matches!(
        secure_boot
            .reset_keys(SecureBootResetKeysType::DeletePk)
            .await?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[tokio::test]
async fn pk_upload_and_reset_return_task_monitors() -> Result<(), Box<dyn StdError>> {
    // BlueField-3 BMC BF-25.10-20 queues both operations for the next boot and
    // answers each with 202, a Task Monitor Location, and Retry-After: 30.
    let bmc = Arc::new(Bmc::default());
    let secure_boot = secure_boot(
        bmc.clone(),
        json!({ "SecureBootDatabases": { ODATA_ID: DATABASES_ID } }),
    )
    .await?;
    bmc.expect(Expect::get(
        DATABASES_ID,
        json!({
            ODATA_ID: DATABASES_ID,
            ODATA_TYPE: "#SecureBootDatabaseCollection.SecureBootDatabaseCollection",
            "Name": "UEFI SecureBoot Database Collection",
            "Members": [{ ODATA_ID: PK_DATABASE_ID }],
            "Members@odata.count": 1
        }),
    ));
    let databases = secure_boot
        .databases()
        .await?
        .expect("SecureBootDatabases is advertised");
    let reset_target = format!("{PK_DATABASE_ID}/Actions/SecureBootDatabase.ResetKeys");
    bmc.expect(Expect::get(
        PK_DATABASE_ID,
        json!({
            ODATA_ID: PK_DATABASE_ID,
            ODATA_TYPE: "#SecureBootDatabase.v1_0_1.SecureBootDatabase",
            "Actions": {
                "#SecureBootDatabase.ResetKeys": { "target": &reset_target }
            },
            "Certificates": { ODATA_ID: CERTIFICATES_ID },
            "DatabaseId": "PK",
            "Id": "PK",
            "Name": "PK Database"
        }),
    ));
    let pk = databases
        .members()
        .await?
        .pop()
        .expect("one database is returned");

    bmc.expect(Expect::action_task(
        &reset_target,
        json!({ "ResetKeysType": "DeleteAllKeys" }),
        task_monitor("/redfish/v1/TaskService/Tasks/2/Monitor"),
    ));
    let ModificationResponse::Task(reset) = pk
        .reset_keys(SecureBootDatabaseResetKeysType::DeleteAllKeys)
        .await?
    else {
        return Err("expected ResetKeys to return a Task".into());
    };
    assert_eq!(
        reset.location.0.to_string(),
        "/redfish/v1/TaskService/Tasks/2/Monitor"
    );

    bmc.expect(Expect::get(
        CERTIFICATES_ID,
        json!({
            ODATA_ID: CERTIFICATES_ID,
            ODATA_TYPE: "#CertificateCollection.CertificateCollection",
            "Name": "Certificate Collection",
            "Members": [],
            "Members@odata.count": 0
        }),
    ));
    let certificates = pk
        .certificates()
        .await?
        .expect("Certificates is advertised");
    bmc.expect(Expect::create_task(
        CERTIFICATES_ID,
        json!({ "CertificateString": "pem-data", "CertificateType": "PEM" }),
        task_monitor("/redfish/v1/TaskService/Tasks/3/Monitor"),
    ));
    let create = CertificateCreate::builder("pem-data".to_string(), CertificateType::Pem).build();
    let ModificationResponse::Task(upload) = certificates.create(&create).await? else {
        return Err("expected the upload to return a Task".into());
    };
    assert_eq!(
        upload.location.0.to_string(),
        "/redfish/v1/TaskService/Tasks/3/Monitor"
    );
    assert_eq!(upload.retry_after, Some(Duration::from_secs(30)));

    Ok(())
}

fn task_monitor(location: &str) -> AsyncTask {
    AsyncTask {
        location: ODataId::from(location.to_string()).into(),
        retry_after: Some(Duration::from_secs(30)),
        task: None,
    }
}

async fn secure_boot(bmc: Arc<Bmc>, fields: Value) -> Result<SecureBoot<Bmc>, Box<dyn StdError>> {
    let root_id = ODataId::service_root();
    bmc.expect(Expect::get(
        &root_id,
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: "#ServiceRoot.v1_19_0.ServiceRoot",
            "Id": "RootService",
            "Name": "RootService",
            "ProtocolFeaturesSupported": {
                "ExpandQuery": {
                    "NoLinks": true
                }
            },
            "Systems": { ODATA_ID: SYSTEMS_ID },
            "Links": {
                "Sessions": { ODATA_ID: "/redfish/v1/SessionService/Sessions" }
            }
        }),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;

    bmc.expect(Expect::expand(
        SYSTEMS_ID,
        json!({
            ODATA_ID: SYSTEMS_ID,
            ODATA_TYPE: "#ComputerSystemCollection.ComputerSystemCollection",
            "Name": "Computer System Collection",
            "Members": [{
                ODATA_ID: SYSTEM_ID,
                ODATA_TYPE: "#ComputerSystem.v1_25_0.ComputerSystem",
                "Id": "System_0",
                "Name": "System_0",
                "SecureBoot": { ODATA_ID: SECURE_BOOT_ID },
                "Status": {
                    "Health": "OK",
                    "State": "Enabled"
                }
            }]
        }),
    ));
    let mut systems = root
        .systems()
        .await?
        .expect("Systems is advertised")
        .members()
        .await?;
    let system = systems.pop().expect("one system is returned");

    let base = json!({
        ODATA_ID: SECURE_BOOT_ID,
        ODATA_TYPE: "#SecureBoot.v1_1_0.SecureBoot",
        "Id": "SecureBoot",
        "Name": "UEFI Secure Boot"
    });
    bmc.expect(Expect::get(SECURE_BOOT_ID, json_merge([&base, &fields])));
    system
        .secure_boot()
        .await?
        .ok_or_else(|| "SecureBoot is advertised".into())
}

fn certificate(id: &str, certificate_id: &str) -> Value {
    json!({
        ODATA_ID: id,
        ODATA_TYPE: "#Certificate.v1_7_0.Certificate",
        "CertificateString": "pem-data",
        "CertificateType": "PEM",
        "Id": certificate_id,
        "Name": format!("{certificate_id} certificate")
    })
}
