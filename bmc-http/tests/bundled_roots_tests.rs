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

//! The rustls client trusts the bundled Mozilla roots, so it builds on a host
//! without CA certificates. On Linux, `SSL_CERT_FILE` replaces the system CA
//! bundle, so an empty file stands for such a host. The test sets the process
//! environment, so it is a test binary of its own.

#![cfg(all(target_os = "linux", feature = "reqwest"))]

use std::env;
use std::error::Error;
use std::fs;
use std::path::Path;

use nv_redfish_bmc_http::reqwest::Client;
#[cfg(not(feature = "reqwest-native-tls"))]
use nv_redfish_bmc_http::reqwest::ClientParams;

fn remove_system_ca_certificates() -> Result<(), Box<dyn Error>> {
    let bundle = Path::new(env!("CARGO_TARGET_TMPDIR")).join("empty-ca-bundle.pem");
    fs::write(&bundle, "")?;
    env::set_var("SSL_CERT_FILE", &bundle);
    env::remove_var("SSL_CERT_DIR");
    Ok(())
}

#[test]
fn rustls_client_builds_without_system_ca_certificates() -> Result<(), Box<dyn Error>> {
    remove_system_ca_certificates()?;

    Client::new()?;
    Ok(())
}

#[cfg(not(feature = "reqwest-native-tls"))]
#[test]
fn rustls_only_client_ignores_use_rust_tls_false() -> Result<(), Box<dyn Error>> {
    remove_system_ca_certificates()?;

    Client::with_params(ClientParams {
        use_rust_tls: false,
        ..ClientParams::default()
    })?;
    Ok(())
}
