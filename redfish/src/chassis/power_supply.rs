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

use crate::resource::ResetType;
use crate::schema::power_supply::PowerSupply as PowerSupplySchema;
use crate::schema::power_supply_metrics::PowerSupplyMetrics;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

#[cfg(feature = "sensors")]
use crate::extract_sensor_uris;
#[cfg(feature = "oem-delta")]
use crate::oem::delta::DeltaPowerSupply;
#[cfg(feature = "sensors")]
use crate::sensor::SensorLink;
#[cfg(feature = "oem-delta")]
use std::convert::identity;

/// Represents a power supply in a chassis.
///
/// Provides access to power supply information and associated metrics/sensors.
pub struct PowerSupply<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<PowerSupplySchema>,
}

impl<B: Bmc> PowerSupply<B> {
    /// Create a new power supply handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<PowerSupplySchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this power supply.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<PowerSupplySchema> {
        self.data.clone()
    }

    /// Reset this power supply.
    ///
    /// # Errors
    ///
    /// Returns an error if the power supply does not support the `Reset`
    /// action or if invoking the action fails.
    pub async fn reset(
        &self,
        reset_type: Option<ResetType>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;

        if actions.reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        actions
            .reset(self.bmc.as_ref(), reset_type)
            .await
            .map_err(Error::Bmc)
    }

    /// Get power supply metrics.
    ///
    /// Returns the power supply's performance and state metrics if available.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The power supply does not have metrics
    /// - Fetching metrics data fails
    pub async fn metrics(&self) -> Result<Option<Arc<PowerSupplyMetrics>>, Error<B>> {
        if let Some(metrics_ref) = &self.data.metrics {
            metrics_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get the metrics sensors for this power supply.
    ///
    /// Returns a vector of `Sensor<B>` obtained from metrics metrics, if available.
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
                        single: input_voltage,
                        single: input_current_amps,
                        single: input_power_watts,
                        single: energyk_wh,
                        single: frequency_hz,
                        single: output_power_watts,
                        single: temperature_celsius,
                        single: fan_speed_percent,
                        vec: rail_voltage,
                        vec: rail_current_amps,
                        vec: rail_power_watts,
                        vec: fan_speeds_percent
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

    /// Delta Energy Systems OEM extension for this power supply.
    ///
    /// Delta power shelves report per-PSU power state under
    /// `Oem/deltaenergysystems` rather than the standard `PowerState` field.
    ///
    /// Returns `Ok(None)` when the power supply does not include Delta OEM
    /// extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing the Delta OEM data fails.
    #[cfg(feature = "oem-delta")]
    pub fn oem_delta(&self) -> Result<Option<DeltaPowerSupply<B>>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map(DeltaPowerSupply::new)
            .transpose()
            .map(|v| v.and_then(identity))
    }
}
