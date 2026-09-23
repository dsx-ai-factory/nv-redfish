// SPDX-FileCopyrightText: Copyright (c) 2025-2026 MIRANTIS, INC. & AFFILIATES. All rights reserved.
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

pub use serde_json::Value;

pub(crate) fn fix_ntp_null_elements(mut json: Value) -> Value {
    if let Value::Object(ref mut obj) = json {
        if let Some(Value::Object(ref mut ntp)) = obj.get_mut("NTP") {
            if let Some(Value::Array(ref mut ntp_servers)) = ntp.get_mut("NTPServers") {
                for server in ntp_servers {
                    if server.is_null() {
                        *server = Value::String("".to_string());
                    }
                }
            }
        }
    }
    json
}
