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

//! OData identifiers used by generated types
//!
//! Minimal wrappers for Redfish/OData identifiers used throughout generated code:
//! - [`ODataId`]: value of `@odata.id`, the canonical resource path (opaque string)
//! - [`ODataETag`]: value of `@odata.etag`, the HTTP entity tag (opaque string)
//!
//! Notes
//! - These types are intentionally semantic‑unaware; they do not validate content.
//! - [`ODataId::service_root()`] returns the conventional Redfish service root path.
//! - Formatting/Display returns the raw underlying string.
//!
//! Example
//! ```rust
//! use nv_redfish_core::ODataId;
//!
//! let root = ODataId::service_root();
//! assert_eq!(root.to_string(), "/redfish/v1");
//! ```
//!
//! References:
//! - OASIS OData 4.01 — `@odata.id`, `@odata.etag`
//! - DMTF Redfish Specification DSP0266 — `https://www.dmtf.org/standards/redfish`
//!

use core::fmt::Display;
use core::fmt::Formatter;
use core::fmt::Result as FmtResult;
use serde::Deserialize;
use serde::Serialize;

/// Type for `@odata.id` identifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ODataId(String);

impl ODataId {
    /// Redfish service root id.
    #[must_use]
    pub fn service_root() -> Self {
        Self("/redfish/v1".into())
    }

    /// Last segment of `ODataId`.
    ///
    /// # Examples
    /// * `"/redfish/v1/Systems/1" -> Some("1")`
    /// * `"/redfish/v1/Systems/1/" -> Some("1")`
    /// * `"redfish" -> Some("redfish")`
    /// * `"" -> None`
    /// * `"/" -> None`
    #[must_use]
    pub fn last_segment(&self) -> Option<&str> {
        let path = self.0.trim_end_matches('/');
        path.rsplit_once('/')
            .map(|(_, v)| v)
            .or_else(|| (!path.is_empty()).then_some(path))
    }

    /// Returns whether this path is a segment-aware prefix of another path.
    ///
    /// Equal paths return `true`.
    ///
    /// `"/redfish/v1/TaskService/Tasks"` is a prefix for
    /// `"/redfish/v1/TaskService/Tasks/42"`, but not
    /// `"/redfish/v1/TaskService/TasksExtra/42"`.
    #[must_use]
    pub fn is_path_prefix(&self, other: &Self) -> bool {
        let prefix = self.0.trim_end_matches('/');
        if prefix.is_empty() {
            return self.0.starts_with('/') && other.0.starts_with('/');
        }

        let Some(suffix) = other.0.strip_prefix(prefix) else {
            return false;
        };

        suffix.is_empty() || suffix.starts_with('/')
    }
}

impl From<String> for ODataId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl Display for ODataId {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

/// Type for `@odata.etag` identifier.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ODataETag(String);

impl From<String> for ODataETag {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl Display for ODataETag {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

/// Type for retrieving `@odata.type` from a JSON payload.
pub struct ODataType<'a> {
    /// Namespace of the data type. For example: `["Chassis", "v1_22_0"]`.
    pub namespace: Vec<&'a str>,
    /// Name of the type. For example "Chassis".
    pub type_name: &'a str,
}

impl ODataType<'_> {
    /// Get `@odata.type` from a JSON payload and parse it.
    #[must_use]
    pub fn parse_from(v: &serde_json::Value) -> Option<ODataType<'_>> {
        v.get("@odata.type")
            .and_then(|v| v.as_str())
            .and_then(|v| v.strip_prefix('#'))
            .and_then(|v| {
                let mut all = v.split('.').collect::<Vec<_>>();
                all.pop().map(|type_name| ODataType {
                    namespace: all,
                    type_name,
                })
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_from_returns_none_for_empty_odata_type() {
        let value = serde_json::json!({ "@odata.type": "" });

        let odata_type = ODataType::parse_from(&value);

        assert!(odata_type.is_none());
    }

    #[test]
    fn last_segment_returns_last_path_segment() {
        let id = ODataId("/redfish/v1/Systems/1".into());
        assert_eq!(id.last_segment(), Some("1"));
    }

    #[test]
    fn last_segment_ignores_trailing_slash() {
        let id = ODataId("/redfish/v1/Systems/1/".into());
        assert_eq!(id.last_segment(), Some("1"));
    }

    #[test]
    fn last_segment_handles_multiple_trailing_slashes() {
        let id = ODataId("/redfish/v1/Systems/1///".into());
        assert_eq!(id.last_segment(), Some("1"));
    }

    #[test]
    fn last_segment_returns_none_for_empty_string() {
        let id = ODataId("".into());
        assert_eq!(id.last_segment(), None);
    }

    #[test]
    fn last_segment_returns_none_for_root_path() {
        let id = ODataId("/".into());
        assert_eq!(id.last_segment(), None);
    }

    #[test]
    fn last_segment_returns_none_for_multiple_root_slashes() {
        let id = ODataId("///".into());
        assert_eq!(id.last_segment(), None);
    }

    #[test]
    fn last_segment_returns_segment_for_single_component_relative_path() {
        let id = ODataId("redfish".into());
        assert_eq!(id.last_segment(), Some("redfish"));
    }

    #[test]
    fn last_segment_returns_last_segment_for_relative_path() {
        let id = ODataId("redfish/v1/Systems/1".into());
        assert_eq!(id.last_segment(), Some("1"));
    }

    #[test]
    fn last_segment_handles_leading_slash_before_single_segment() {
        let id = ODataId("/redfish".into());
        assert_eq!(id.last_segment(), Some("redfish"));
    }

    #[test]
    fn service_root_last_segment_is_v1() {
        assert_eq!(ODataId::service_root().last_segment(), Some("v1"));
    }

    #[test]
    fn is_path_prefix_accepts_child_path() {
        let prefix = ODataId("/redfish/v1/TaskService/Tasks".into());
        let id = ODataId("/redfish/v1/TaskService/Tasks/42".into());

        assert!(prefix.is_path_prefix(&id));
    }

    #[test]
    fn is_path_prefix_accepts_prefix_with_trailing_slash() {
        let prefix = ODataId("/redfish/v1/TaskService/Tasks/".into());
        let id = ODataId("/redfish/v1/TaskService/Tasks/42".into());

        assert!(prefix.is_path_prefix(&id));
    }

    #[test]
    fn is_path_prefix_rejects_matching_string_without_segment_boundary() {
        let prefix = ODataId("/redfish/v1/TaskService/Tasks".into());
        let id = ODataId("/redfish/v1/TaskService/TasksExtra/42".into());

        assert!(!prefix.is_path_prefix(&id));
    }

    #[test]
    fn is_path_prefix_accepts_exact_path_without_trailing_slash() {
        let prefix = ODataId("/redfish/v1/TaskService/Tasks".into());
        let id = ODataId("/redfish/v1/TaskService/Tasks".into());

        assert!(prefix.is_path_prefix(&id));
    }

    #[test]
    fn is_path_prefix_accepts_root_path() {
        let prefix = ODataId("/".into());
        let id = ODataId("/".into());

        assert!(prefix.is_path_prefix(&id));
    }

    #[test]
    fn is_path_prefix_accepts_root_child_path() {
        let prefix = ODataId("/".into());
        let id = ODataId("/redfish".into());

        assert!(prefix.is_path_prefix(&id));
    }
}
