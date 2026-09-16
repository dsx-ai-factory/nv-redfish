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

//! Processor and its configuration.

use crate::computer_system::processor_metrics::ProcessorMetrics;
use crate::schema::processor::Processor as ProcessorSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

#[cfg(feature = "controls")]
use crate::control::Control;
#[cfg(feature = "environment-metrics")]
use crate::environment_metrics::EnvironmentMetrics;
#[cfg(feature = "sensors")]
use crate::extract_sensor_uris;
#[cfg(feature = "sensors")]
use crate::sensor::SensorLink;

/// Represents a processor in a computer system.
///
/// Provides access to processor information and associated metrics/sensors.
pub struct Processor<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<ProcessorSchema>,
}

impl<B: Bmc> Processor<B> {
    /// Create a new processor handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ProcessorSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this processor.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<ProcessorSchema> {
        self.data.clone()
    }

    /// Get processor metrics.
    ///
    /// Returns `Ok(None)` when the `Metrics` link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching metrics data fails.
    pub async fn metrics(&self) -> Result<Option<ProcessorMetrics<B>>, Error<B>> {
        if let Some(metrics_ref) = &self.data.metrics {
            ProcessorMetrics::new(&self.bmc, metrics_ref)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get the environment metrics of this processor.
    ///
    /// Returns `Ok(None)` when the `EnvironmentMetrics` link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching environment metrics data fails.
    #[cfg(feature = "environment-metrics")]
    pub async fn environment_metrics(&self) -> Result<Option<EnvironmentMetrics<B>>, Error<B>> {
        if let Some(env_ref) = &self.data.environment_metrics {
            EnvironmentMetrics::new(&self.bmc, env_ref).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get the environment sensors for this processor.
    ///
    /// Returns a vector of `Sensor<B>` obtained from environment metrics, if available.
    ///
    /// # Errors
    ///
    /// Returns an error if get of environment metrics failed.
    #[cfg(feature = "sensors")]
    pub async fn environment_sensor_links(&self) -> Result<Vec<SensorLink<B>>, Error<B>> {
        Ok(self
            .environment_metrics()
            .await?
            .map(|metrics| metrics.sensor_links())
            .unwrap_or_default())
    }

    /// Get the environment power limit control for this processor.
    ///
    /// Returns `Ok(None)` when environment metrics or `PowerLimitWatts` is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching environment metrics or the control fails.
    #[cfg(feature = "controls")]
    pub async fn environment_power_limit_control(&self) -> Result<Option<Control<B>>, Error<B>> {
        let Some(metrics) = self.environment_metrics().await? else {
            return Ok(None);
        };

        metrics.power_limit_control().await
    }

    /// Get the metrics sensors for this processor.
    ///
    /// Returns a vector of `Sensor<B>` obtained from metrics metrics, if available.
    ///
    /// # Errors
    ///
    /// Returns an error if get of metrics failed.
    #[cfg(feature = "sensors")]
    pub async fn metrics_sensor_links(&self) -> Result<Vec<SensorLink<B>>, Error<B>> {
        let sensor_refs = if let Some(metrics_ref) = &self.data.metrics {
            metrics_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)
                .map(|m| {
                    extract_sensor_uris!(m,
                        single: core_voltage,
                    )
                })?
        } else {
            Vec::new()
        };

        Ok(sensor_refs
            .into_iter()
            .map(|r| SensorLink::new(&self.bmc, r))
            .collect())
    }
}
