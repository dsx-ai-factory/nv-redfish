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

//! List the UEFI Secure Boot platform key (PK) certificates on a live BMC
//! and optionally upload one.

use std::error::Error as StdError;
use std::fs;
use std::io::Error as IoError;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::reqwest::ClientParams;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::certificate::CertificateCreate;
use nv_redfish::certificate::CertificateType;
use nv_redfish::core::ModificationResponse;
use nv_redfish::ServiceRoot;
use url::Url;

const PLATFORM_KEY_DATABASE: &str = "PK";

#[derive(Debug, Parser)]
#[command(about = "List Secure Boot PK certificates and optionally upload one")]
struct Args {
    /// BMC base URL, for example https://10.0.0.1
    #[arg(long)]
    bmc: Url,

    #[arg(long)]
    username: String,

    #[arg(long)]
    password: String,

    /// PEM certificate to upload to an empty PK database.
    #[arg(long)]
    pem: Option<PathBuf>,

    /// ComputerSystem Id to use. Defaults to the first system.
    #[arg(long)]
    system: Option<String>,

    /// Accept invalid TLS certificates.
    #[arg(long, default_value_t = false)]
    insecure: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError>> {
    let args = Args::parse();

    let client = Client::with_params(ClientParams::new().accept_invalid_certs(args.insecure))?;
    let bmc = Arc::new(HttpBmc::new(
        client,
        args.bmc,
        BmcCredentials::new(args.username, args.password),
        CacheSettings::with_capacity(0),
    ));
    let root = ServiceRoot::new(bmc.clone()).await?;

    let systems = root
        .systems()
        .await?
        .ok_or_else(|| IoError::other("Systems is not advertised"))?
        .members()
        .await?;
    let system = match &args.system {
        Some(id) => systems.iter().find(|system| system.raw().id == id.as_str()),
        None => systems.first(),
    }
    .ok_or_else(|| IoError::other("requested system was not found"))?;

    let secure_boot = system
        .secure_boot()
        .await?
        .ok_or_else(|| IoError::other("SecureBoot is not advertised"))?;
    println!(
        "System {}: SecureBootEnable={:?} SecureBootCurrentBoot={:?}",
        system.raw().id,
        secure_boot.secure_boot_enable(),
        secure_boot.secure_boot_current_boot()
    );

    let databases = secure_boot
        .databases()
        .await?
        .ok_or_else(|| IoError::other("SecureBootDatabases is not advertised"))?
        .members()
        .await?;
    let pk = databases
        .iter()
        .find(|database| database.raw().id == PLATFORM_KEY_DATABASE)
        .ok_or_else(|| IoError::other("PK database was not found"))?;
    let certificates = pk
        .certificates()
        .await?
        .ok_or_else(|| IoError::other("PK database does not advertise Certificates"))?;

    let members = certificates.members().await?;
    println!("PK certificates: {}", members.len());
    for certificate in &members {
        let certificate = certificate.raw();
        let subject = certificate
            .subject
            .as_ref()
            .and_then(|subject| subject.common_name.as_deref());
        println!(
            "  id={} type={:?} subject={subject:?}",
            certificate.id, certificate.certificate_type
        );
    }

    let Some(pem) = args.pem else {
        return Ok(());
    };
    if !members.is_empty() {
        println!("PK already has a certificate; delete it with ResetKeys before uploading.");
        return Ok(());
    }

    let create = CertificateCreate::builder(fs::read_to_string(pem)?, CertificateType::Pem).build();
    match certificates.create(&create).await? {
        ModificationResponse::Entity(certificate) => {
            println!("Uploaded certificate {}", certificate.raw().id);
        }
        ModificationResponse::Task(task) => println!(
            "Upload accepted; track it at {} (Retry-After {:?}). \
             BlueField applies the key on the next boot.",
            task.location.0, task.retry_after
        ),
        ModificationResponse::Message(_) | ModificationResponse::Empty => {
            println!("Upload accepted");
        }
    }
    Ok(())
}
