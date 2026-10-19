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

//! Shared readers for the inline `Oem.<Vendor>` objects every vendor
//! extension starts from.

use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;
use nv_redfish_core::odata::ODataType;
use nv_redfish_core::Bmc;
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

/// Read a resource's `Oem.<key>` object as `T`.
///
/// The key is usually a vendor name, but anything a vendor nests in
/// the `Oem` object qualifies -- AMI, for one, puts a `ConfigBMC`
/// reference next to its own key.
///
/// Returns `Ok(None)` when the payload carries no object under
/// `key`, including when it carries an explicit `null` -- a shape
/// some firmware uses to mean "no extension", which must not read as a
/// parse failure.
///
/// Only for OEM objects with a single concrete shape. When
/// `@odata.type` selects between shapes, test it with [`declares`] and
/// resolve the variant before deserializing.
///
/// # Errors
///
/// Returns an error if the object does not parse as `T`.
pub fn oem_object<T, B>(oem: &ResourceOemSchema, key: &str) -> Result<Option<Arc<T>>, Error<B>>
where
    T: for<'de> Deserialize<'de>,
    B: Bmc,
{
    let Some(value) = oem_value(oem, key) else {
        return Ok(None);
    };
    T::deserialize(value)
        .map(|parsed| Some(Arc::new(parsed)))
        .map_err(Error::Json)
}

/// The raw `Oem.<key>` object, when the payload carries one.
///
/// Treats an explicit `null` as absence, exactly as [`oem_object`]
/// does, so presence checks and parses agree on what "no extension"
/// looks like.
#[must_use]
pub fn oem_value<'a>(oem: &'a ResourceOemSchema, key: &str) -> Option<&'a Value> {
    oem.additional_properties
        .get(key)
        .filter(|value| !value.is_null())
}

/// Whether an OEM object declares `@odata.type` with this top-level
/// namespace and type name.
///
/// Reads `false` when `@odata.type` is absent or unparseable. It only
/// selects between shapes, so what an undeclared payload means is the
/// caller's decision, not this function's.
#[must_use]
pub fn declares(value: &Value, namespace: &str, type_name: &str) -> bool {
    ODataType::parse_from(value).is_some_and(|odata_type| {
        odata_type.type_name == type_name && odata_type.namespace.first() == Some(&namespace)
    })
}
