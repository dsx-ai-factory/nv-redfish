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

use crate::bmc_quirks::BmcQuirks;
use crate::entity_link::FromLink;
use crate::hardware_id::HardwareIdRef;
use crate::hardware_id::Manufacturer as HardwareIdManufacturer;
use crate::hardware_id::Model as HardwareIdModel;
use crate::hardware_id::PartNumber as HardwareIdPartNumber;
use crate::hardware_id::SerialNumber as HardwareIdSerialNumber;
use crate::patch_support::JsonValue;
use crate::patch_support::Payload;
use crate::patch_support::ReadPatchFn;
use crate::resource::ResetType;
use crate::schema::chassis::Chassis as ChassisSchema;
use crate::Error;
use crate::NvBmc;
use crate::Resource;
use crate::ResourceSchema;
use nv_redfish_core::bmc::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::future::Future;
use std::sync::Arc;

#[cfg(feature = "assembly")]
use crate::assembly::Assembly;
#[cfg(feature = "network-adapters")]
use crate::chassis::NetworkAdapter;
#[cfg(feature = "network-adapters")]
use crate::chassis::NetworkAdapterCollection;
#[cfg(feature = "power")]
use crate::chassis::Power;
#[cfg(feature = "power-supplies")]
use crate::chassis::PowerSupply;
#[cfg(feature = "thermal")]
use crate::chassis::Thermal;
#[cfg(feature = "controls")]
use crate::control::Control;
#[cfg(feature = "controls")]
use crate::control::ControlCollection;
#[cfg(feature = "environment-metrics")]
use crate::environment_metrics::EnvironmentMetrics;
#[cfg(feature = "log-services")]
use crate::log_service::LogService;
#[cfg(all(feature = "oem-liteon", feature = "power-supplies"))]
use crate::oem::liteon;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaCbcChassis;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaChassisActions;
#[cfg(feature = "pcie-devices")]
use crate::pcie_device::PcieDeviceCollection;
#[cfg(feature = "sensors")]
use crate::schema::sensor::Sensor as SchemaSensor;
#[cfg(feature = "sensors")]
use crate::sensor::SensorLink;
#[cfg(feature = "oem-nvidia")]
use std::convert::identity;

#[doc(hidden)]
pub enum ChassisTag {}

/// Chassis manufacturer.
pub type Manufacturer<T> = HardwareIdManufacturer<T, ChassisTag>;

/// Chassis model.
pub type Model<T> = HardwareIdModel<T, ChassisTag>;

/// Chassis part number.
pub type PartNumber<T> = HardwareIdPartNumber<T, ChassisTag>;

/// Chassis serial number.
pub type SerialNumber<T> = HardwareIdSerialNumber<T, ChassisTag>;

pub struct Config {
    pub read_patch_fn: Option<ReadPatchFn>,
}

impl Config {
    pub fn new(quirks: &BmcQuirks) -> Self {
        let mut patches = Vec::new();
        if quirks.bug_invalid_contained_by_fields() {
            patches.push(remove_invalid_contained_by_fields as fn(JsonValue) -> JsonValue);
        }
        if quirks.bug_missing_chassis_type_field() {
            patches.push(add_default_chassis_type);
        }
        if quirks.bug_missing_chassis_name_field() {
            patches.push(add_default_chassis_name);
        }
        if quirks.bug_empty_uuid_field() {
            patches.push(normalize_empty_uuid_field);
        }
        let read_patch_fn = (!patches.is_empty())
            .then(|| Arc::new(move |v| patches.iter().fold(v, |acc, f| f(acc))) as ReadPatchFn);
        Self { read_patch_fn }
    }
}

/// Represents a chassis in the BMC.
///
/// Provides access to chassis information and sub-resources such as power supplies.
pub struct Chassis<B: Bmc> {
    #[allow(dead_code)] // used if any feature enabled.
    bmc: NvBmc<B>,
    data: Arc<ChassisSchema>,
    #[allow(dead_code)] // used when assembly feature enabled.
    config: Arc<Config>,
}

impl<B: Bmc> Chassis<B> {
    /// Create a new chassis handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ChassisSchema>,
    ) -> Result<Self, Error<B>> {
        let config = Config::new(&bmc.quirks);
        if let Some(read_patch_fn) = &config.read_patch_fn {
            Payload::get(bmc.as_ref(), nav, read_patch_fn.as_ref()).await
        } else {
            nav.get(bmc.as_ref()).await.map_err(Error::Bmc)
        }
        .map(|data| Self {
            bmc: bmc.clone(),
            data,
            config: config.into(),
        })
    }

    /// Get the raw schema data for this chassis.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<ChassisSchema> {
        self.data.clone()
    }

    /// Reset this chassis.
    ///
    /// # Errors
    ///
    /// Returns an error if the chassis does not support the `Reset` action or
    /// if invoking the action fails.
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

    /// Get hardware identifier of the network adpater.
    #[must_use]
    pub fn hardware_id(&self) -> HardwareIdRef<'_, ChassisTag> {
        HardwareIdRef {
            manufacturer: self
                .data
                .manufacturer
                .as_ref()
                .and_then(Option::as_deref)
                .map(Manufacturer::new),
            model: self
                .data
                .model
                .as_ref()
                .and_then(Option::as_deref)
                .map(Model::new),
            part_number: self
                .data
                .part_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(PartNumber::new),
            serial_number: self
                .data
                .serial_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(SerialNumber::new),
        }
    }

    /// Get assembly of this chassis
    ///
    /// Returns `Ok(None)` when the assembly link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching assembly data fails.
    #[cfg(feature = "assembly")]
    pub async fn assembly(&self) -> Result<Option<Assembly<B>>, Error<B>> {
        if let Some(assembly_ref) = &self.data.assembly {
            Assembly::new(&self.bmc, assembly_ref).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get power supplies from this chassis.
    ///
    /// Attempts to fetch power supplies from `PowerSubsystem` (modern API)
    /// with fallback to Power resource (deprecated API).
    ///
    /// # Errors
    ///
    /// Returns an error if fetching power supply data fails.
    #[cfg(feature = "power-supplies")]
    pub async fn power_supplies(&self) -> Result<Vec<PowerSupply<B>>, Error<B>> {
        if let Some(ps) = &self.data.power_subsystem {
            let ps = ps.get(self.bmc.as_ref()).await.map_err(Error::Bmc)?;
            if let Some(supplies) = &ps.power_supplies {
                let supplies = &self.bmc.expand_property(supplies).await?.members;
                let mut power_supplies = Vec::with_capacity(supplies.len());
                for power_supply in supplies {
                    power_supplies.push(PowerSupply::new(&self.bmc, power_supply).await?);
                }
                return Ok(power_supplies);
            }
        }

        Ok(Vec::new())
    }

    /// Get LiteOn OEM power supplies from this chassis.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching power supply data fails.
    #[cfg(all(feature = "oem-liteon", feature = "power-supplies"))]
    pub async fn oem_liteon_power_supply_links(
        &self,
    ) -> Result<Option<Vec<liteon::power_supply::LiteonPowerSupplyLink<B>>>, Error<B>> {
        liteon::power_supply::chassis_fetch_links(&self.bmc, self).await
    }

    /// Get legacy Power resource (for older BMCs).
    ///
    /// Returns the deprecated `Chassis/Power` resource if available.
    /// For modern BMCs, prefer using direct sensor links via `HasSensors`
    /// or the modern `PowerSubsystem` API.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching power data fails.
    #[cfg(feature = "power")]
    pub async fn power(&self) -> Result<Option<Power<B>>, Error<B>> {
        if let Some(power_ref) = &self.data.power {
            Ok(Some(Power::new(&self.bmc, power_ref).await?))
        } else {
            Ok(None)
        }
    }

    /// Get controls for this chassis.
    ///
    /// Returns `Ok(None)` when the controls link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching controls data fails.
    #[cfg(feature = "controls")]
    pub async fn controls(&self) -> Result<Option<Vec<Control<B>>>, Error<B>> {
        let Some(controls_ref) = &self.data.controls else {
            return Ok(None);
        };

        ControlCollection::new(&self.bmc, controls_ref)
            .await?
            .members()
            .await
            .map(Some)
    }

    /// Get legacy Thermal resource (for older BMCs).
    ///
    /// Returns the deprecated `Chassis/Thermal` resource if available.
    /// For modern BMCs, prefer using direct sensor links via `HasSensors`
    /// or the modern `ThermalSubsystem` API.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching thermal data fails.
    #[cfg(feature = "thermal")]
    pub async fn thermal(&self) -> Result<Option<Thermal<B>>, Error<B>> {
        if let Some(thermal_ref) = &self.data.thermal {
            Thermal::new(&self.bmc, thermal_ref).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get network adapter resources
    ///
    /// Returns the `Chassis/NetworkAdapter` resources if available, and `Ok(None)` when
    /// the network adapters link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching network adapters data fails.
    #[cfg(feature = "network-adapters")]
    pub async fn network_adapters(&self) -> Result<Option<Vec<NetworkAdapter<B>>>, Error<B>> {
        if let Some(network_adapters_collection_ref) = &self.data.network_adapters {
            NetworkAdapterCollection::new(&self.bmc, network_adapters_collection_ref)
                .await?
                .members()
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get log services for this chassis.
    ///
    /// Returns `Ok(None)` when the log services link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching log service data fails.
    #[cfg(feature = "log-services")]
    pub async fn log_services(&self) -> Result<Option<Vec<LogService<B>>>, Error<B>> {
        if let Some(log_services_ref) = &self.data.log_services {
            let log_services_collection = log_services_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?;

            let mut log_services = Vec::new();
            for m in &log_services_collection.members {
                log_services.push(LogService::new(&self.bmc, m).await?);
            }

            Ok(Some(log_services))
        } else {
            Ok(None)
        }
    }

    /// Get the environment sensors for this chassis.
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

    /// Get the environment metrics of this chassis.
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

    /// Get the environment power limit control for this chassis.
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

    /// Get the sensors collection for this chassis.
    ///
    /// Returns all available sensors associated with the chassis, and `Ok(None)`
    /// when the sensors link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching sensors data fails.
    #[cfg(feature = "sensors")]
    pub async fn sensor_links(&self) -> Result<Option<Vec<SensorLink<B>>>, Error<B>> {
        if let Some(sensors_collection) = &self.data.sensors {
            let sc = sensors_collection
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?;
            let mut sensor_data = Vec::with_capacity(sc.members.len());
            for sensor in &sc.members {
                sensor_data.push(SensorLink::new(
                    &self.bmc,
                    NavProperty::<SchemaSensor>::new_reference(sensor.id().clone()),
                ));
            }
            Ok(Some(sensor_data))
        } else {
            Ok(None)
        }
    }

    /// Get `PCIe` devices for this computer system.
    ///
    /// Returns `Ok(None)` when the `PCIeDevices` link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching `PCIe` devices data fails.
    #[cfg(feature = "pcie-devices")]
    pub async fn pcie_devices(&self) -> Result<Option<PcieDeviceCollection<B>>, crate::Error<B>> {
        if let Some(p) = &self.data.pcie_devices {
            PcieDeviceCollection::new(&self.bmc, p).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// NVIDIA CBC chassis OEM extension
    ///
    /// Returns `Ok(None)` when the chassis does not include NVIDIA OEM extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM data parsing fails.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia_cbc(&self) -> Result<Option<NvidiaCbcChassis<B>>, Error<B>> {
        self.data
            .base
            .base
            .oem
            .as_ref()
            .map(NvidiaCbcChassis::new)
            .transpose()
            .map(|v| v.and_then(identity))
    }

    /// Get the NVIDIA OEM actions advertised by this chassis.
    ///
    /// Returns `Ok(None)` when the chassis has no OEM actions object.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM actions cannot be parsed.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia_actions(&self) -> Result<Option<NvidiaChassisActions<B>>, Error<B>> {
        self.data
            .actions
            .as_ref()
            .and_then(|actions| actions.oem.as_ref())
            .map(|actions| NvidiaChassisActions::new(&self.bmc, actions))
            .transpose()
    }
}

impl<B: Bmc> Resource for Chassis<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}

impl<B: Bmc> FromLink<B> for Chassis<B> {
    type Schema = ChassisSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}

fn remove_invalid_contained_by_fields(mut v: JsonValue) -> JsonValue {
    if let JsonValue::Object(ref mut obj) = v {
        if let Some(JsonValue::Object(ref mut links_obj)) = obj.get_mut("Links") {
            if let Some(JsonValue::Object(ref mut contained_by_obj)) =
                links_obj.get_mut("ContainedBy")
            {
                contained_by_obj.retain(|k, _| k == "@odata.id");
            }
        }
    }
    v
}

fn add_default_chassis_type(v: JsonValue) -> JsonValue {
    if let JsonValue::Object(mut obj) = v {
        obj.entry("ChassisType")
            .or_insert(JsonValue::String("Other".into()));
        JsonValue::Object(obj)
    } else {
        v
    }
}

fn add_default_chassis_name(v: JsonValue) -> JsonValue {
    if let JsonValue::Object(mut obj) = v {
        obj.entry("Name")
            .or_insert(JsonValue::String("Unnamed chassis".into()));
        JsonValue::Object(obj)
    } else {
        v
    }
}

fn normalize_empty_uuid_field(mut v: JsonValue) -> JsonValue {
    if let JsonValue::Object(ref mut obj) = v {
        if let Some(uuid) = obj.get_mut("UUID") {
            let is_empty = uuid.as_str().is_some_and(str::is_empty);
            if is_empty {
                *uuid = JsonValue::Null;
            }
        }
    }
    v
}
