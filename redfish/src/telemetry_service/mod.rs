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

//! Telemetry Service entities and helpers.
//!
//! This module provides typed access to Redfish `TelemetryService`.

mod metric_definition;
mod metric_report;
mod metric_report_definition;

use crate::entity_link::EntityLink;
use crate::schema::metric_definition::MetricDefinition as MetricDefinitionSchema;
use crate::schema::metric_report::MetricReport as MetricReportSchema;
use crate::schema::metric_report_definition::MetricReportDefinition as MetricReportDefinitionSchema;
use crate::schema::telemetry_service::TelemetryService as TelemetryServiceSchema;
use crate::schema::telemetry_service::TelemetryServiceUpdate;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

#[doc(inline)]
pub use metric_definition::MetricDefinition;
#[doc(inline)]
pub use metric_definition::MetricDefinitionCreate;
#[doc(inline)]
pub use metric_definition::MetricDefinitionUpdate;
#[doc(inline)]
pub use metric_report::MetricReport;
#[doc(inline)]
pub use metric_report_definition::MetricReportDefinition;
#[doc(inline)]
pub use metric_report_definition::MetricReportDefinitionCreate;
#[doc(inline)]
pub use metric_report_definition::MetricReportDefinitionType;
#[doc(inline)]
pub use metric_report_definition::MetricReportDefinitionUpdate;
#[doc(inline)]
pub use metric_report_definition::ReportActionsEnum;
#[doc(inline)]
pub use metric_report_definition::Wildcard;
#[doc(inline)]
pub use metric_report_definition::WildcardUpdate;

/// Metric report entity wrapper.
pub type MetricReportLink<B> = EntityLink<B, MetricReportSchema>;

/// Telemetry service.
///
/// Provides access to metric reports and metric definitions.
pub struct TelemetryService<B: Bmc> {
    data: Arc<TelemetryServiceSchema>,
    bmc: NvBmc<B>,
}

impl<B: Bmc> TelemetryService<B> {
    /// Create a new telemetry service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        if let Some(service_ref) = &root.root.telemetry_service {
            let data = service_ref.get(bmc.as_ref()).await.map_err(Error::Bmc)?;
            Ok(Some(Self {
                data,
                bmc: bmc.clone(),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the raw schema data for this telemetry service.
    #[must_use]
    pub fn raw(&self) -> Arc<TelemetryServiceSchema> {
        self.data.clone()
    }

    /// Enable or disable telemetry service.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated telemetry service.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if updating telemetry service fails.
    pub async fn set_enabled(&self, enabled: bool) -> Result<ModificationResponse<Self>, Error<B>> {
        let update = TelemetryServiceUpdate::builder()
            .with_service_enabled(enabled)
            .build();

        self.bmc
            .as_ref()
            .update::<_, NavProperty<TelemetryServiceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                &update,
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move {
                let data = nav.get(self.bmc.as_ref()).await.map_err(Error::Bmc)?;

                Ok(Self {
                    data,
                    bmc: self.bmc.clone(),
                })
            })
            .await
    }

    /// Get `Vec<MetricReportLink>` associated with this telemetry service.
    ///
    /// Fetches the metric report collection and returns a list of
    /// [`MetricReportLink`] handles.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the telemetry service does not expose a `MetricReports` collection
    /// - retrieving the collection fails
    pub async fn metric_report_links(&self) -> Result<Option<Vec<MetricReportLink<B>>>, Error<B>> {
        if let Some(collection_ref) = &self.data.metric_reports {
            let collection = collection_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?;

            let mut items = Vec::with_capacity(collection.members.len());
            for m in &collection.members {
                items.push(MetricReportLink::new(
                    &self.bmc,
                    NavProperty::new_reference(m.id().clone()),
                ));
            }

            Ok(Some(items))
        } else {
            Ok(None)
        }
    }

    /// Get `Vec<MetricDefinition>` associated with this telemetry service.
    ///
    /// Fetches the metric definition collection and returns a list of [`MetricDefinition`] handles.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the telemetry service does not expose a `MetricDefinitions` collection
    /// - retrieving the collection fails
    pub async fn metric_definitions(&self) -> Result<Option<Vec<MetricDefinition<B>>>, Error<B>> {
        if let Some(collection_ref) = &self.data.metric_definitions {
            let collection = self.bmc.expand_property(collection_ref).await?;

            let mut items = Vec::with_capacity(collection.members.len());
            for m in &collection.members {
                items.push(MetricDefinition::new(&self.bmc, m).await?);
            }

            Ok(Some(items))
        } else {
            Ok(None)
        }
    }

    /// Get `Vec<MetricReportDefinition>` associated with this telemetry service.
    ///
    /// Fetches the metric report definition collection and returns a list of
    /// [`MetricReportDefinition`] handles.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the telemetry service does not expose a `MetricReportDefinitions` collection
    /// - retrieving the collection fails
    pub async fn metric_report_definitions(
        &self,
    ) -> Result<Option<Vec<MetricReportDefinition<B>>>, Error<B>> {
        if let Some(collection_ref) = &self.data.metric_report_definitions {
            let collection = self.bmc.expand_property(collection_ref).await?;

            let mut items = Vec::with_capacity(collection.members.len());
            for m in &collection.members {
                items.push(MetricReportDefinition::new(&self.bmc, m).await?);
            }

            Ok(Some(items))
        } else {
            Ok(None)
        }
    }

    /// Create a metric definition.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the created metric definition.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the telemetry service does not expose a `MetricDefinitions` collection
    /// - creating the entity fails
    pub async fn create_metric_definition(
        &self,
        create: &MetricDefinitionCreate,
    ) -> Result<ModificationResponse<MetricDefinition<B>>, Error<B>> {
        let collection_ref = self
            .data
            .metric_definitions
            .as_ref()
            .ok_or(Error::MetricDefinitionsNotAvailable)?;

        self.bmc
            .as_ref()
            .create::<_, NavProperty<MetricDefinitionSchema>>(collection_ref.id(), create)
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { MetricDefinition::new(&self.bmc, &nav).await })
            .await
    }

    /// Create a metric report definition.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the created metric report
    ///   definition.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - the telemetry service does not expose a `MetricReportDefinitions` collection
    /// - creating the entity fails
    pub async fn create_metric_report_definition(
        &self,
        create: &MetricReportDefinitionCreate,
    ) -> Result<ModificationResponse<MetricReportDefinition<B>>, Error<B>> {
        let collection_ref = self
            .data
            .metric_report_definitions
            .as_ref()
            .ok_or(Error::MetricReportDefinitionsNotAvailable)?;

        self.bmc
            .as_ref()
            .create::<_, NavProperty<MetricReportDefinitionSchema>>(collection_ref.id(), create)
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move {
                MetricReportDefinition::new(&self.bmc, &nav).await
            })
            .await
    }
}
