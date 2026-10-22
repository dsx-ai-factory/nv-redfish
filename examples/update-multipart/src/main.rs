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
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use futures_util::io::AllowStdIo;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::reqwest::ClientParams;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::core::DataStream;
use nv_redfish::core::UploadStream;
use nv_redfish::update_service::MultipartUpdateParameters;
use nv_redfish::ServiceRoot;
use url::Url;

#[derive(Debug, Parser)]
#[command()]
struct Args {
    #[arg(long)]
    bmc: Url,

    #[arg(long)]
    username: String,

    #[arg(long)]
    password: String,

    #[arg(long)]
    file: PathBuf,

    #[arg(long = "target")]
    targets: Vec<String>,

    #[arg(long, default_value_t = false)]
    force_update: bool,

    #[arg(long, default_value_t = false)]
    http_push_uri: bool,

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
        CacheSettings::default(),
    ));

    let root = ServiceRoot::new(Arc::clone(&bmc)).await?;
    let update_service = root
        .update_service()
        .await?
        .ok_or("BMC did not expose UpdateService")?;

    let firmware = std::fs::File::open(&args.file)?;
    let content_length = firmware.metadata()?.len();

    if args.http_push_uri {
        let update_stream =
            UploadStream::new(AllowStdIo::new(firmware)).with_content_length(content_length);
        let response = update_service
            .http_push_uri_update_from_reader::<_, serde_json::Value>(
                update_stream,
                Duration::from_secs(1800),
            )
            .await?;

        println!("{response:#?}");

        return Ok(());
    }

    let file_name = args
        .file
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("firmware path does not have a valid file name")?
        .to_string();
    let update_stream =
        DataStream::new(file_name, AllowStdIo::new(firmware)).with_content_length(content_length);

    let parameters = MultipartUpdateParameters::builder()
        .with_force_update(args.force_update)
        .with_targets(args.targets)
        .build();

    let response = update_service
        .multipart_update_from_reader::<_, _, serde_json::Value>(
            &parameters,
            update_stream,
            Duration::from_secs(1800),
        )
        .await?;

    println!("{response:#?}");

    Ok(())
}
