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

use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;

/// Redfish `LocationIndicatorActive` (`Edm.Boolean` or vendor LED map).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LocationIndicatorActive {
    /// Standard Redfish boolean indicator.
    Boolean(bool),
    /// Lite-On-style LED states on some power-supply firmware.
    LedIndicators {
        /// Fault LED state (e.g. `"OFF"`).
        #[serde(rename = "FaultLed")]
        fault_led: String,
        /// Power LED state (e.g. `"Solid"`).
        #[serde(rename = "PowerLed")]
        power_led: String,
    },
}

/// Deserialize an optional nullable field. nv-redfish models these fields
/// with `Option<Option<T>>`, where `None` means "no field" and
/// `Some(None)` means the field is explicitly set to null.
///
/// # Errors
///
/// Returns an error if deserialization of the underlying type fails.
pub fn de_optional_nullable<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(de).map(Some)
}

/// Deserialize a required nullable field. nv-redfish models these fields
/// with `Option<T>`, where `None` means null.
///
/// # Errors
///
/// Returns an error if deserialization of the underlying type fails.
pub fn de_required_nullable<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Deserialize::deserialize(de)
}

#[cfg(test)]
mod location_indicator_active_tests {
    use super::LocationIndicatorActive;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Deserialize)]
    struct Wrapper {
        #[serde(
            rename = "LocationIndicatorActive",
            default,
            deserialize_with = "crate::deserialize::de_optional_nullable"
        )]
        location_indicator_active: Option<Option<LocationIndicatorActive>>,
    }

    #[test]
    fn stores_led_object_and_boolean() {
        let led: Wrapper = serde_json::from_value(json!({
            "LocationIndicatorActive": { "FaultLed": "OFF", "PowerLed": "Solid" }
        }))
        .expect("led object");
        assert_eq!(
            led.location_indicator_active,
            Some(Some(LocationIndicatorActive::LedIndicators {
                fault_led: "OFF".into(),
                power_led: "Solid".into(),
            }))
        );

        let boolean: Wrapper =
            serde_json::from_value(json!({ "LocationIndicatorActive": false })).expect("boolean");
        assert_eq!(
            boolean.location_indicator_active,
            Some(Some(LocationIndicatorActive::Boolean(false)))
        );
    }
}
