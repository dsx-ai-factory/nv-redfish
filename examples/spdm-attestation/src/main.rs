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

use std::error::Error as StdError;
use std::fs::File;
use std::io::Error as IoError;
use std::io::Read as _;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::reqwest::ClientParams;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::core::ModificationResponse;
use nv_redfish::schema::task::TaskState;
use nv_redfish::task_service::task_payload_location;
use nv_redfish::ServiceRoot;
use url::Url;

#[derive(Debug, Parser)]
#[command(about = "Collect SPDM signed measurements from a live BMC")]
struct Args {
    #[arg(long)]
    bmc: Url,

    #[arg(long)]
    username: String,

    #[arg(long)]
    password: String,

    #[arg(long)]
    component: String,

    #[arg(long, default_value_t = false)]
    insecure: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn StdError>> {
    let Args {
        bmc,
        username,
        password,
        component: requested_component,
        insecure,
    } = Args::parse();
    let nonce = generate_nonce()?;

    let client = Client::with_params(ClientParams::new().accept_invalid_certs(insecure))?;
    let bmc = Arc::new(HttpBmc::new(
        client,
        bmc,
        BmcCredentials::new(username, password),
        CacheSettings::with_capacity(0),
    ));
    let root = ServiceRoot::new(bmc).await?;
    let collection = root
        .component_integrity()
        .await?
        .ok_or_else(|| IoError::other("ComponentIntegrity is not advertised"))?;
    let components = collection.members().await?;

    println!("Discovered ComponentIntegrity members:");
    for component in &components {
        let raw = component.raw();
        println!(
            "  id={} enabled={:?} type={:?} version={} action={}",
            raw.id,
            raw.component_integrity_enabled,
            raw.component_integrity_type,
            raw.component_integrity_type_version,
            component.spdm_get_signed_measurements_target().is_some()
        );
    }

    let selected = components
        .iter()
        .position(|component| component.raw().id == requested_component.as_str())
        .ok_or_else(|| {
            IoError::other(format!(
                "requested component {:?} was not found",
                requested_component
            ))
        })?;
    let component = &components[selected];

    println!("Selected component: {}", component.raw().id);
    if let Some(certificate) = component.component_certificate().await? {
        let certificate = certificate.raw();
        println!(
            "Responder certificate: id={} type={:?} slot={:?}",
            certificate.id,
            certificate.certificate_type,
            certificate
                .spdm
                .as_ref()
                .and_then(|spdm| spdm.slot_id.as_ref())
                .and_then(Option::as_ref)
        );
    } else {
        println!("Responder certificate: not advertised");
    }

    let response = component
        .spdm_get_signed_measurements(Some(nonce), None, None)
        .await?;
    let task = match response {
        ModificationResponse::Entity(evidence) => {
            print_evidence(
                &evidence.hashing_algorithm,
                &evidence.signing_algorithm,
                &evidence.version,
                evidence.signed_measurements.len(),
            );
            return Ok(());
        }
        ModificationResponse::Task(task) => task,
        ModificationResponse::Empty => {
            return Err(IoError::other("action completed without evidence or a Task").into());
        }
    };

    println!("Task monitor: {}", task.location.0);
    println!(
        "Task resource: {}",
        task.task_resource
            .as_ref()
            .map_or_else(|| "<not returned>".to_string(), ToString::to_string)
    );

    let task_service = root
        .task_service()
        .await?
        .ok_or_else(|| IoError::other("TaskService is not advertised"))?;
    let task_link = task_service.task_link(&task)?;
    let poll_interval = task.retry_after.unwrap_or(Duration::from_secs(5));
    let mut completed = false;
    let mut result_location = None;

    for poll in 1..=30 {
        let resource = task_link.fetch().await?;
        println!(
            "Poll {poll}: state={:?} status={:?} percent={:?}",
            resource.task_state,
            resource.task_status,
            resource.percent_complete.flatten()
        );

        if resource.task_state == Some(TaskState::Completed) {
            completed = true;
            result_location = task_payload_location(&resource);
            break;
        }
        if matches!(
            resource.task_state,
            Some(TaskState::Killed | TaskState::Exception | TaskState::Cancelled)
        ) {
            return Err(IoError::other(format!(
                "measurement Task failed in state {:?}",
                resource.task_state
            ))
            .into());
        }
        tokio::time::sleep(poll_interval).await;
    }

    if !completed {
        return Err(IoError::other("measurement Task did not complete after 30 polls").into());
    }

    if let Some(result_location) = &result_location {
        println!("Evidence location: {result_location}");
    } else {
        println!("Task did not expose an evidence Location; using NVIDIA fallback");
    }
    let evidence = component
        .spdm_signed_measurements_data(result_location.as_ref())
        .await?;
    let evidence = evidence.raw();
    print_evidence(
        &evidence.hashing_algorithm,
        &evidence.signing_algorithm,
        &evidence.version,
        evidence.signed_measurements.len(),
    );

    Ok(())
}

fn print_evidence(
    hashing_algorithm: &str,
    signing_algorithm: &str,
    version: &str,
    evidence_bytes: usize,
) {
    println!("SPDM evidence:");
    println!("  version: {version}");
    println!("  hashing algorithm: {hashing_algorithm}");
    println!("  signing algorithm: {signing_algorithm}");
    println!("  signed measurements length: {evidence_bytes}");
}

fn generate_nonce() -> Result<String, IoError> {
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut nonce = String::with_capacity(bytes.len() * 2);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        nonce.push(char::from(HEX[usize::from(byte >> 4)]));
        nonce.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    Ok(nonce)
}
