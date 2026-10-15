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

//! Shared serialization primitives for generated Redfish server models.

use std::collections::BTreeMap;

use serde::{Serialize, Serializer};

/// Raw JSON is confined to schema-authorized extension properties.
pub use serde_json::Value as JsonValue;

/// An EDM GUID, serialized as a UUID string.
pub use uuid::Uuid as EdmGuid;

/// Extension properties serialized alongside a schema's known fields.
pub type AdditionalProperties = BTreeMap<String, JsonValue>;

/// A closed schema enum with its exact CSDL wire spelling.
pub trait Enum: Copy {
    /// Return the wire spelling of this enumerator.
    fn as_str(self) -> &'static str;
}

/// A finite JSON floating-point number. NaN and infinity are rejected at construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FiniteF64(f64);

/// A non-finite number cannot be represented as a JSON number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonFiniteNumber;

impl std::fmt::Display for NonFiniteNumber {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JSON numbers must be finite")
    }
}

impl std::error::Error for NonFiniteNumber {}

impl std::convert::TryFrom<f64> for FiniteF64 {
    type Error = NonFiniteNumber;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        if value.is_finite() {
            Ok(Self(value))
        } else {
            Err(NonFiniteNumber)
        }
    }
}

impl FiniteF64 {
    /// Recover the finite primitive without conversion.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Serialize for FiniteF64 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.0)
    }
}

/// A link to another resource, independent of its expanded representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reference {
    /// URI of the referenced resource.
    #[serde(rename = "@odata.id")]
    pub odata_id: String,
}

impl Reference {
    /// Construct a reference from its required URI.
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            odata_id: uri.into(),
        }
    }
}

impl From<String> for Reference {
    fn from(uri: String) -> Self {
        Self::new(uri)
    }
}

impl From<&str> for Reference {
    fn from(uri: &str) -> Self {
        Self::new(uri)
    }
}

impl From<&String> for Reference {
    fn from(uri: &String) -> Self {
        Self::new(uri.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::{FiniteF64, Reference};
    use serde_json::json;

    #[test]
    fn references_serialize_as_links_without_expanded_resources() {
        let uri = String::from("/redfish/v1/Chassis/chassis0");
        let reference = Reference::from(&uri);
        assert_eq!(
            serde_json::to_value(reference).unwrap(),
            json!({"@odata.id": uri})
        );
    }

    #[test]
    fn finite_numbers_serialize_as_numbers_and_reject_non_finite_inputs() {
        let value = FiniteF64::try_from(42.5).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), json!(42.5));
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(FiniteF64::try_from(value).is_err());
        }
    }
}
