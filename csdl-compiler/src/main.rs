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

use std::io;
use std::io::Write as _;

use nv_redfish_csdl_compiler::commands::process_command;
use nv_redfish_csdl_compiler::commands::Commands;
use nv_redfish_csdl_compiler::Error;

use clap::Parser;

/// Compiler CLI.
#[derive(Parser, Debug)]
#[command(name = "csdl-compiler")]
#[command(about = "Redfish schema CSDL compiler", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

fn main() -> Result<(), Error> {
    let cli = Cli::parse();

    let messages = process_command(&cli.command)?;

    let mut stdout = io::stdout().lock();

    messages
        .into_iter()
        .try_for_each(|message| writeln!(stdout, "{message}"))
        .and_then(|()| stdout.flush())
        .map_err(|error| Error::Io("stdout".into(), error))
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::CommandFactory as _;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
