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

//! Log Service entities and collections.
//!
//! This module provides types for working with Redfish LogService resources
//! and their log entries.

use crate::schema::log_entry::LogEntry;
use crate::schema::log_service::LogService as LogServiceSchema;
use crate::Error;
use crate::NvBmc;
use crate::Resource;
use crate::ResourceSchema;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

/// Log service.
///
/// Provides functions to access log entries and perform log operations.
pub struct LogService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<LogServiceSchema>,
}

impl<B: Bmc> LogService<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<LogServiceSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this log service.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<LogServiceSchema> {
        self.data.clone()
    }

    /// List all log entries.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The log service does not have a log entries collection
    /// - Fetching log entries data fails
    pub async fn entries(&self) -> Result<Option<Vec<Arc<LogEntry>>>, Error<B>> {
        if let Some(entries_ref) = &self.data.entries {
            let entries_collection = self.bmc.expand_property(entries_ref).await?;
            self.expand_entries(&entries_collection.members)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Filter log entries using `OData` filter query.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The log service does not have a log entries collection
    /// - Filtering log entries data fails
    pub async fn filter_entries(
        &self,
        filter: nv_redfish_core::FilterQuery,
    ) -> Result<Option<Vec<Arc<LogEntry>>>, Error<B>> {
        if let Some(entries_ref) = &self.data.entries {
            let entries_collection = entries_ref
                .filter(self.bmc.as_ref(), filter)
                .await
                .map_err(Error::Bmc)?;

            self.expand_entries(&entries_collection.members)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Clear all log entries.
    ///
    /// # Arguments
    ///
    /// * `log_entry_codes` - Optional log entry codes to clear specific entries
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The log service does not support the `ClearLog` action
    /// - The action execution fails
    pub async fn clear_log(
        &self,
        log_entry_codes: Option<String>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;

        actions
            .clear_log(self.bmc.as_ref(), log_entry_codes)
            .await
            .map_err(Error::Bmc)
    }

    /// This unwraps `NavProperty`, usually all BMC already have them expanded, so we do not expect network IO here
    async fn expand_entries(
        &self,
        entry_refs: &[NavProperty<LogEntry>],
    ) -> Result<Vec<Arc<LogEntry>>, Error<B>> {
        let mut entries = Vec::new();
        for entry_ref in entry_refs {
            let entry = entry_ref.get(self.bmc.as_ref()).await.map_err(Error::Bmc)?;
            entries.push(entry);
        }
        Ok(entries)
    }
}

impl<B: Bmc> Resource for LogService<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}

#[cfg(all(test, feature = "resource-serialization"))]
mod tests {
    use crate::schema::log_entry::LogEntry;

    #[test]
    fn log_entry_serialization_preserves_retained_wire_properties() {
        let value = serde_json::json!({
            "@odata.id": "/redfish/v1/Systems/System/LogServices/EventLog/Entries/1",
            "Id": "1",
            "Name": "Event log entry",
            "EntryType": "Event",
            "MessageId": "ResourceEvent.1.0.ResourceErrorsDetected",
            "DiagnosticDataType": "CPER",
            "DiagnosticData": null,
            "Actions": {},
            "CPER": {
                "NotificationType": null,
                "Oem": {
                    "Vendor": {
                        "RecordType": "Example"
                    }
                }
            },
            "Links": {
                "OriginOfCondition": {
                    "@odata.id": "/redfish/v1/Systems/System"
                }
            },
            "Oem": {
                "Vendor": {
                    "ErrorId": "EXAMPLE-ERROR"
                }
            }
        });

        let entry: LogEntry =
            serde_json::from_value(value.clone()).expect("log entry must deserialize");

        let serialized = serde_json::to_value(entry).expect("log entry must serialize");

        assert_eq!(serialized, value);
    }

    #[test]
    fn log_entry_serialization_normalizes_unknown_enum_values() {
        let value = serde_json::json!({
            "@odata.id": "/redfish/v1/Systems/System/LogServices/EventLog/Entries/1",
            "Id": "1",
            "Name": "Event log entry",
            "EntryType": "FutureEntryType"
        });

        let entry: LogEntry = serde_json::from_value(value).expect("log entry must deserialize");

        let serialized = serde_json::to_value(entry).expect("log entry must serialize");

        assert_eq!(
            serialized.get("EntryType"),
            Some(&serde_json::json!("UnsupportedValue"))
        );
    }
}
