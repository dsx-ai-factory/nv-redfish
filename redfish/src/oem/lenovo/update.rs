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

//! Shared Lenovo OEM update composition.

use crate::schema::resource::OemUpdate;
use serde::Serialize;
use serde_json::Value;

const OEM_KEY: &str = "Lenovo";

/// Wrap a typed Lenovo value in the standard Redfish OEM update container.
pub fn oem_update<T: Serialize>(
    existing: Option<OemUpdate>,
    update: &T,
) -> Result<OemUpdate, serde_json::Error> {
    let mut additional_properties = existing
        .and_then(|oem| oem.additional_properties.as_object().cloned())
        .unwrap_or_default();
    additional_properties.insert(OEM_KEY.to_string(), serde_json::to_value(update)?);
    Ok(OemUpdate {
        additional_properties: Value::Object(additional_properties),
    })
}
