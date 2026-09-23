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

//! A document read as it came off the wire, before any repair or typed
//! deserialization.

use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::Expandable;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value;

/// A Redfish document as JSON.
///
/// It satisfies the bounds a [`Bmc`] read needs, so any transport can fetch
/// one where it would fetch a typed resource, and it reports the document's
/// `@odata.etag`, so a caching transport revalidates it exactly as it would
/// a typed one.
///
/// [`Bmc`]: nv_redfish_core::Bmc
#[derive(Debug)]
pub struct Raw {
    id: ODataId,
    etag: Option<ODataETag>,
    value: Value,
}

impl Raw {
    /// The document.
    #[must_use]
    pub const fn value(&self) -> &Value {
        &self.value
    }

    /// The document, owned.
    #[must_use]
    pub fn into_value(self) -> Value {
        self.value
    }
}

impl EntityTypeRef for Raw {
    fn odata_id(&self) -> &ODataId {
        &self.id
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.etag.as_ref()
    }
}

impl Expandable for Raw {}

impl<'de> Deserialize<'de> for Raw {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let id = text("@odata.id").unwrap_or_default().into();
        let etag = text("@odata.etag").map(ODataETag::from);
        Ok(Self { id, etag, value })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_raw_document_carries_its_identity_and_etag() {
        let raw: Raw = serde_json::from_value(json!({
            "@odata.id": "/redfish/v1/Chassis/1U",
            "@odata.etag": "W/\"9F8E7D\"",
            "Id": "1U",
        }))
        .expect("any object is a raw document");
        assert_eq!(raw.odata_id().to_string(), "/redfish/v1/Chassis/1U");
        assert_eq!(
            raw.etag(),
            Some(&ODataETag::from("W/\"9F8E7D\"".to_owned()))
        );
        assert_eq!(raw.value()["Id"], "1U");

        // A document without either still reads; the transport's cache
        // simply has nothing to revalidate.
        let bare: Raw =
            serde_json::from_value(json!({ "Members": [] })).expect("any object is a raw document");
        assert!(bare.etag().is_none());
        assert_eq!(bare.into_value()["Members"], json!([]));
    }
}
