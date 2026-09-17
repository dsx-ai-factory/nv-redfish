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

//! Walk the metric resources of a live BMC and report what its NVIDIA
//! OEM extensions actually look like.
//!
//! The schemas behind these extensions are vendored from CSDL rather
//! than derived from captured responses, so the open question on real
//! hardware is whether firmware sends what the CSDL declares. This
//! walker answers it by probing every resource the crate can reach and
//! separating the three outcomes that matter:
//!
//! * `link absent` -- the resource does not link the sub-resource. Not
//!   interesting; plenty of platforms omit them.
//! * `oem absent` -- the resource exists but carries no `Oem.Nvidia`.
//!   Also fine, and the shape an explicit `null` reduces to.
//! * `parse error` -- firmware sent an `Oem.Nvidia` the schema cannot
//!   read. This is the bug class worth finding, so any occurrence makes
//!   the process exit non-zero.
//!
//! It never stops at the first failure: one unreadable processor must
//! not hide what the other sixty report.

use clap::Parser;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::reqwest::ClientParams;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::environment_metrics::EnvironmentMetrics;
use nv_redfish::oem::nvidia::NvidiaProcessorMetrics;
use nv_redfish::oem::nvidia::OEM_KEY;
use nv_redfish::oem::oem_value;
use nv_redfish::schema::resource::Oem as ResourceOem;
use nv_redfish::telemetry_service::MetricReport;
use nv_redfish::Error;
use nv_redfish::ServiceRoot;
use serde_json::Value;
use std::error::Error as StdError;
use std::process::ExitCode;
use std::sync::Arc;
use url::Url;

#[derive(Debug, Parser)]
#[command(about = "Probe NVIDIA OEM metric extensions on a live BMC")]
struct Args {
    /// Base URL of the BMC, e.g. https://10.0.0.1
    #[arg(long)]
    bmc: Url,

    #[arg(long)]
    username: String,

    #[arg(long)]
    password: String,

    /// Accept self-signed certificates, as most BMCs present.
    #[arg(long, default_value_t = false)]
    insecure: bool,

    /// Print the raw `Oem.Nvidia` object for every extension found.
    ///
    /// This is the payload to diff against the vendored CSDL when a
    /// property is missing or reads as `None`.
    #[arg(long, default_value_t = false)]
    dump: bool,
}

/// What a single `oem_nvidia()` probe produced.
enum Outcome {
    /// Extension read successfully. Carries a one-line description.
    Read(String),
    /// Resource present, no NVIDIA extension on it.
    OemAbsent,
    /// The resource itself is not linked.
    LinkAbsent,
    /// The resource was reachable but something failed: a parse
    /// failure on the OEM object, or the fetch itself.
    Failed(String),
}

#[derive(Default)]
struct Tally {
    read: usize,
    oem_absent: usize,
    link_absent: usize,
    failed: usize,
}

impl Tally {
    /// Record and print one probe.
    fn note(&mut self, indent: &str, label: &str, outcome: &Outcome) {
        let rendered = match outcome {
            Outcome::Read(detail) => {
                self.read += 1;
                format!("ok -- {detail}")
            }
            Outcome::OemAbsent => {
                self.oem_absent += 1;
                "oem absent".to_owned()
            }
            Outcome::LinkAbsent => {
                self.link_absent += 1;
                "link absent".to_owned()
            }
            Outcome::Failed(err) => {
                self.failed += 1;
                format!("FAILED -- {err}")
            }
        };
        println!("{indent}{label:<28}: {rendered}");
    }
}

/// Describe the `Oem.Nvidia` object a resource carries.
///
/// Reports the `@odata.type` firmware declared and how many properties
/// it sent, which together say whether the payload matches the shape
/// the vendored CSDL expects.
fn describe(oem: Option<&ResourceOem>, dump: bool) -> String {
    let Some(nvidia) = oem.and_then(|oem| oem_value(oem, OEM_KEY)) else {
        // oem_nvidia() returned Some, so the object is there; only a
        // caller passing the wrong Oem bag reaches this.
        return "extension present, raw object not reachable".to_owned();
    };

    let odata_type = nvidia
        .get("@odata.type")
        .and_then(Value::as_str)
        .unwrap_or("<no @odata.type>");
    let properties = nvidia.as_object().map_or(0, serde_json::Map::len);

    let mut detail = format!("{odata_type} ({properties} properties)");
    if dump {
        let body = serde_json::to_string_pretty(nvidia)
            .unwrap_or_else(|err| format!("<unprintable: {err}>"));
        detail.push('\n');
        detail.push_str(&body);
    }
    detail
}

#[tokio::main]
async fn main() -> Result<ExitCode, Box<dyn StdError>> {
    let args = Args::parse();

    let client = Client::with_params(ClientParams::new().accept_invalid_certs(args.insecure))?;
    let bmc = Arc::new(HttpBmc::new(
        client,
        args.bmc,
        BmcCredentials::new(args.username, args.password),
        CacheSettings::default(),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    println!(
        "Service root: vendor {:?}, product {:?}",
        root.vendor(),
        root.product()
    );

    let mut tally = Tally::default();
    walk_systems(&root, &mut tally, args.dump).await?;
    walk_chassis(&root, &mut tally, args.dump).await?;
    walk_telemetry(&root, &mut tally, args.dump).await?;

    println!();
    println!(
        "extensions read: {}, oem absent: {}, link absent: {}, failed: {}",
        tally.read, tally.oem_absent, tally.link_absent, tally.failed
    );

    if tally.failed > 0 {
        println!();
        println!("Some resources failed to read. Re-run with --dump to capture");
        println!("the payloads, then compare them against schema/oem/nvidia.");
        return Ok(ExitCode::FAILURE);
    }
    Ok(ExitCode::SUCCESS)
}

/// Processors, their metrics, memory devices and drives.
async fn walk_systems(
    root: &ServiceRoot<HttpBmc<Client>>,
    tally: &mut Tally,
    dump: bool,
) -> Result<(), Box<dyn StdError>> {
    println!();
    println!("=== Systems ===");
    let Some(systems) = root.systems().await? else {
        println!("service root exposes no Systems collection");
        return Ok(());
    };

    for system in systems.members().await? {
        println!("system {}", system.raw().id);

        if let Some(processors) = system.processors().await? {
            for processor in processors {
                println!("  processor {}", processor.raw().id);
                let outcome = match processor.metrics().await {
                    Ok(Some(metrics)) => match metrics.oem_nvidia() {
                        Ok(Some(oem)) => {
                            let raw = metrics.raw();
                            let shape = match oem {
                                NvidiaProcessorMetrics::Gpu(_) => "Gpu",
                                NvidiaProcessorMetrics::Generic(_) => "Generic",
                            };
                            Outcome::Read(format!(
                                "read as {shape}; {}",
                                describe(raw.oem.as_ref(), dump)
                            ))
                        }
                        Ok(None) => Outcome::OemAbsent,
                        Err(err) => Outcome::Failed(err.to_string()),
                    },
                    Ok(None) => Outcome::LinkAbsent,
                    Err(err) => Outcome::Failed(err.to_string()),
                };
                tally.note("    ", "ProcessorMetrics", &outcome);

                probe_environment(&processor.environment_metrics().await, tally, dump).await;
            }
        }

        if let Some(modules) = system.memory_modules().await? {
            for memory in modules {
                println!("  memory {}", memory.raw().id);
                let outcome = match memory.metrics().await {
                    Ok(Some(metrics)) => match metrics.oem_nvidia() {
                        Ok(Some(_)) => {
                            let raw = metrics.raw();
                            Outcome::Read(describe(raw.oem.as_ref(), dump))
                        }
                        Ok(None) => Outcome::OemAbsent,
                        Err(err) => Outcome::Failed(err.to_string()),
                    },
                    Ok(None) => Outcome::LinkAbsent,
                    Err(err) => Outcome::Failed(err.to_string()),
                };
                tally.note("    ", "MemoryMetrics", &outcome);

                probe_environment(&memory.environment_metrics().await, tally, dump).await;
            }
        }

        if let Some(storages) = system.storage_controllers().await? {
            for storage in storages {
                if let Some(drives) = storage.drives().await? {
                    for drive in drives {
                        println!("  drive {}", drive.raw().id);
                        probe_environment(&drive.environment_metrics().await, tally, dump).await;
                    }
                }
            }
        }
    }
    Ok(())
}

/// Chassis-level environment metrics.
async fn walk_chassis(
    root: &ServiceRoot<HttpBmc<Client>>,
    tally: &mut Tally,
    dump: bool,
) -> Result<(), Box<dyn StdError>> {
    println!();
    println!("=== Chassis ===");
    let Some(collection) = root.chassis().await? else {
        println!("service root exposes no Chassis collection");
        return Ok(());
    };

    for chassis in collection.members().await? {
        println!("chassis {}", chassis.raw().id);
        probe_environment(&chassis.environment_metrics().await, tally, dump).await;
    }
    Ok(())
}

/// Metric reports, reached through the telemetry service.
async fn walk_telemetry(
    root: &ServiceRoot<HttpBmc<Client>>,
    tally: &mut Tally,
    dump: bool,
) -> Result<(), Box<dyn StdError>> {
    println!();
    println!("=== Telemetry ===");
    let Some(service) = root.telemetry_service().await? else {
        println!("service root exposes no TelemetryService");
        return Ok(());
    };
    let Some(links) = service.metric_report_links().await? else {
        println!("telemetry service exposes no MetricReports collection");
        return Ok(());
    };

    println!("{} metric reports", links.len());
    for link in links {
        let id = link.odata_id().clone();
        let upgraded: Result<MetricReport<HttpBmc<Client>>, _> = link.upgrade().await;
        let outcome = match upgraded {
            Ok(report) => match report.oem_nvidia() {
                Ok(Some(_)) => {
                    let raw = report.raw();
                    Outcome::Read(describe(raw.oem.as_ref(), dump))
                }
                Ok(None) => Outcome::OemAbsent,
                Err(err) => Outcome::Failed(err.to_string()),
            },
            Err(err) => Outcome::Failed(err.to_string()),
        };
        tally.note("  ", id.last_segment().unwrap_or("<report>"), &outcome);
    }
    Ok(())
}

/// Report one resource's `EnvironmentMetrics`, plus the sensor links
/// and power-limit control derived from the same fetched body.
async fn probe_environment(
    fetched: &Result<Option<EnvironmentMetrics<HttpBmc<Client>>>, Error<HttpBmc<Client>>>,
    tally: &mut Tally,
    dump: bool,
) {
    let metrics = match fetched {
        Ok(Some(metrics)) => metrics,
        Ok(None) => {
            tally.note("    ", "EnvironmentMetrics", &Outcome::LinkAbsent);
            return;
        }
        Err(err) => {
            tally.note(
                "    ",
                "EnvironmentMetrics",
                &Outcome::Failed(err.to_string()),
            );
            return;
        }
    };

    let outcome = match metrics.oem_nvidia() {
        Ok(Some(_)) => {
            let raw = metrics.raw();
            Outcome::Read(describe(raw.oem.as_ref(), dump))
        }
        Ok(None) => Outcome::OemAbsent,
        Err(err) => Outcome::Failed(err.to_string()),
    };
    tally.note("    ", "EnvironmentMetrics", &outcome);

    // Both are read off the body already fetched above, so they cost
    // no extra GET of the metrics resource itself.
    let sensors = metrics.sensor_links();
    println!("      sensor links            : {}", sensors.len());

    match metrics.power_limit_control().await {
        Ok(Some(control)) => println!("      power limit control     : {}", control.raw().id),
        Ok(None) => println!("      power limit control     : none reported"),
        Err(err) => println!("      power limit control     : FAILED -- {err}"),
    }
}
