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
use std::io::Error as IoError;
use std::io::ErrorKind;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use nv_redfish::bmc_http::reqwest::Client;
use nv_redfish::bmc_http::reqwest::ClientParams;
use nv_redfish::bmc_http::BmcCredentials;
use nv_redfish::bmc_http::CacheSettings;
use nv_redfish::bmc_http::HttpBmc;
use nv_redfish::core::AsyncTask;
use nv_redfish::core::ODataId;
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

    /// Redfish task location returned by an async operation.
    #[arg(long, value_name = "LOCATION")]
    location: String,

    #[arg(long, default_value_t = 1)]
    poll_count: u32,

    #[arg(long, default_value_t = 5)]
    poll_interval_secs: u64,

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
    let task_service = root
        .task_service()
        .await?
        .ok_or_else(|| IoError::new(ErrorKind::NotFound, "TaskService is not available"))?;

    let async_task = AsyncTask {
        location: ODataId::from(args.location).into(),
        task_resource: None,
        retry_after: None,
    };

    let task_link = task_service.task_link(async_task)?;

    for poll in 1..=args.poll_count {
        if poll > 1 {
            tokio::time::sleep(Duration::from_secs(args.poll_interval_secs)).await;
        }

        let task = task_link.fetch().await?;
        let percent_complete = task.percent_complete.flatten();

        println!(
            "Poll {poll}: path={} state={:?} status={:?} percent={:?}",
            task_link.odata_id(),
            task.task_state,
            task.task_status,
            percent_complete
        );

        for message in task
            .messages
            .iter()
            .flatten()
            .filter_map(|message| message.message.as_deref())
        {
            println!("Message: {message}");
        }
    }

    Ok(())
}
