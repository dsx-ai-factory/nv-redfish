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

//! Environment metrics of a resource: power, temperature and humidity.
//!
//! Chassis, processors, memory devices and drives all link to one of
//! these, and their `environment_*` accessors are thin delegations to
//! this type. Reach for those when you have the resource; reach for
//! these methods when you already hold the metrics, so the body is
//! fetched once instead of once per accessor.

#[cfg(feature = "impl-entity-link")]
use crate::entity_link::FromLink;
use crate::schema::environment_metrics::EnvironmentMetrics as EnvironmentMetricsSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
#[cfg(feature = "impl-entity-link")]
use std::future::Future;
use std::sync::Arc;

#[cfg(feature = "controls")]
use crate::control::Control;
#[cfg(feature = "controls")]
use crate::schema::control::Control as ControlSchema;
#[cfg(feature = "controls")]
use nv_redfish_core::ODataId;

#[cfg(feature = "sensors")]
use crate::extract_sensor_uris;
#[cfg(feature = "sensors")]
use crate::sensor::SensorLink;

#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::schema::nvidia_environment_metrics::NvidiaEnvironmentMetrics;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::OEM_KEY;
#[cfg(feature = "oem-nvidia")]
use crate::oem::oem_object;

/// Represents the environment metrics of a resource in the BMC.
pub struct EnvironmentMetrics<B: Bmc> {
    data: Arc<EnvironmentMetricsSchema>,
    bmc: NvBmc<B>,
}

impl<B: Bmc> EnvironmentMetrics<B> {
    /// Create a new environment metrics handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<EnvironmentMetricsSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                data,
                bmc: bmc.clone(),
            })
    }

    /// Sensors whose readings this resource reports.
    ///
    /// Every `DataSourceUri` the metrics carry, as lazy links: the
    /// readings are inlined here, but the sensors themselves are
    /// separate resources.
    #[cfg(feature = "sensors")]
    #[must_use]
    pub fn sensor_links(&self) -> Vec<SensorLink<B>> {
        let metrics = self.data.as_ref();
        extract_sensor_uris!(metrics,
            single: temperature_celsius,
            single: humidity_percent,
            single: power_watts,
            single: energyk_wh,
            single: power_load_percent,
            single: dew_point_celsius,
            single: absolute_humidity,
            single: energy_joules,
            single: ambient_temperature_celsius,
            single: voltage,
            single: current_amps,
            vec: fan_speeds_percent
        )
        .into_iter()
        .map(|nav| SensorLink::new(&self.bmc, nav))
        .collect()
    }

    /// The control behind `PowerLimitWatts`.
    ///
    /// Returns `Ok(None)` when the metrics report no power limit.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the control fails.
    #[cfg(feature = "controls")]
    pub async fn power_limit_control(&self) -> Result<Option<Control<B>>, Error<B>> {
        let Some(Some(uri)) = self
            .data
            .power_limit_watts
            .as_ref()
            .and_then(|control| control.data_source_uri.as_ref())
        else {
            return Ok(None);
        };

        let control_ref = NavProperty::<ControlSchema>::new_reference(ODataId::from(uri.clone()));

        Control::new(&self.bmc, &control_ref).await.map(Some)
    }

    /// Get the raw schema data for these environment metrics.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<EnvironmentMetricsSchema> {
        self.data.clone()
    }

    /// NVIDIA OEM extension.
    ///
    /// Returns `Ok(None)` when the metrics do not include NVIDIA OEM
    /// extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM data parsing fails.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia(&self) -> Result<Option<Arc<NvidiaEnvironmentMetrics>>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, OEM_KEY))
    }
}

#[cfg(feature = "impl-entity-link")]
impl<B: Bmc> FromLink<B> for EnvironmentMetrics<B> {
    type Schema = EnvironmentMetricsSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}
