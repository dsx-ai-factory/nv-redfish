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

use crate::schema::metric_report_definition::MetricReportDefinition as MetricReportDefinitionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

pub use crate::schema::metric_report_definition::MetricReportDefinitionCreate;
pub use crate::schema::metric_report_definition::MetricReportDefinitionType;
pub use crate::schema::metric_report_definition::MetricReportDefinitionUpdate;
pub use crate::schema::metric_report_definition::ReportActionsEnum;
pub use crate::schema::metric_report_definition::Wildcard;
pub use crate::schema::metric_report_definition::WildcardUpdate;

/// Metric report definition entity wrapper.
pub struct MetricReportDefinition<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<MetricReportDefinitionSchema>,
}

impl<B: Bmc> MetricReportDefinition<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<MetricReportDefinitionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get raw metric report definition schema data.
    #[must_use]
    pub fn raw(&self) -> Arc<MetricReportDefinitionSchema> {
        self.data.clone()
    }

    /// Update this metric report definition.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated metric report
    ///   definition.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the entity fails.
    pub async fn update(
        &self,
        update: &MetricReportDefinitionUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<MetricReportDefinitionSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Delete this metric report definition.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the metric report definition
    ///   returned by the server.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting the entity fails.
    pub async fn delete(&self) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .delete::<NavProperty<MetricReportDefinitionSchema>>(self.data.odata_id())
            .await
            .map_err(Error::from)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }
}
