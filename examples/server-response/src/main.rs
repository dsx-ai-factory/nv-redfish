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

//! A chassis with sensors, thermal metrics, and sensor excerpts serialized into stdout.

use std::convert::TryFrom;
use std::io::{self, Write};

use nv_redfish_server_core::{FiniteF64, NonFiniteNumber};
use server_response::schema::chassis::{Chassis, ChassisRequired, ChassisType};
use server_response::schema::chassis_collection::ChassisCollection;
use server_response::schema::physical_context::PhysicalContext;
use server_response::schema::resource::{Health, State, Status};
use server_response::schema::sensor::{ReadingType, Sensor, SensorExcerpt, SensorExcerptArray};
use server_response::schema::sensor_collection::SensorCollection;
use server_response::schema::service_root::{Links, ServiceRoot, ServiceRootRequired};
use server_response::schema::thermal_metrics::{TemperatureSummary, ThermalMetrics};
use server_response::schema::thermal_subsystem::ThermalSubsystem;

const CHASSIS_URI: &str = "/redfish/v1/Chassis/HGX_CPU_0";
const SENSORS_URI: &str = "/redfish/v1/Chassis/HGX_CPU_0/Sensors";

fn service_root_response(
    chassis_collection: &ChassisCollection,
) -> Result<ServiceRoot, Box<dyn std::error::Error>> {
    Ok(ServiceRoot::new(ServiceRootRequired {
        odata_id: "/redfish/v1".into(),
        id: "RootService".into(),
        name: "Root Service".into(),
        links: Links::new("/redfish/v1/SessionService/Sessions"),
    })
    .redfish_version("1.17.0")
    .uuid("bec1c782-162b-4410-891f-4f96bab10237".parse()?)
    .product("GB NVL")
    .vendor("Supermicro")
    .chassis(&chassis_collection.odata_id))
}

fn enabled_status() -> Status {
    Status::new()
        .state(State::Enabled)
        .health(Health::Ok)
        .health_rollup(Health::Ok)
        .conditions([])
}

fn chassis_response(sensors: &SensorCollection, thermal_subsystem: &ThermalSubsystem) -> Chassis {
    Chassis::new(ChassisRequired {
        odata_id: CHASSIS_URI.into(),
        id: "HGX_CPU_0".into(),
        name: "HGX_CPU_0".into(),
        chassis_type: ChassisType::Component,
    })
    .manufacturer("NVIDIA")
    .model("Grace A02P")
    .serial_number("0x000000017844A04120000000120081C0")
    .sensors(&sensors.odata_id)
    .thermal_subsystem(&thermal_subsystem.odata_id)
    .status(enabled_status())
}

fn chassis_collection_response(chassis: &Chassis) -> ChassisCollection {
    ChassisCollection::new(
        "/redfish/v1/Chassis",
        "Chassis Collection",
        [&chassis.odata_id],
    )
    .members_count(1)
}

fn sensor_responses() -> Result<[Sensor; 2], NonFiniteNumber> {
    let temperature_id = "ProcessorModule_0_CPU_0_TempAvg_0";
    let power_id = "ProcessorModule_0_CPU_0_Power_0";
    Ok([
        Sensor::new(
            format!("{SENSORS_URI}/{temperature_id}"),
            temperature_id,
            "ProcessorModule 0 CPU 0 TempAvg 0",
        )
        .reading_type(ReadingType::Temperature)
        .reading(FiniteF64::try_from(34.25)?)
        .reading_units("Cel")
        .reading_range_min(FiniteF64::try_from(-100.0)?)
        .reading_range_max(FiniteF64::try_from(150.0)?)
        .physical_context(PhysicalContext::Cpu)
        .status(enabled_status()),
        Sensor::new(
            format!("{SENSORS_URI}/{power_id}"),
            power_id,
            "ProcessorModule 0 CPU 0 Power 0",
        )
        .reading_type(ReadingType::Power)
        .reading(FiniteF64::try_from(56.315)?)
        .reading_units("W")
        .reading_range_min(FiniteF64::try_from(0.0)?)
        .reading_range_max(FiniteF64::try_from(300.0)?)
        .physical_context(PhysicalContext::Cpu)
        .status(enabled_status()),
    ])
}

fn sensor_collection_response(sensors: &[Sensor]) -> SensorCollection {
    SensorCollection::new(
        SENSORS_URI,
        "Sensors",
        sensors.iter().map(|sensor| &sensor.odata_id),
    )
    .members_count(sensors.len() as u64)
}

fn thermal_metrics_response(temperature: &Sensor) -> ThermalMetrics {
    let reading = SensorExcerptArray {
        reading: temperature.reading,
        ..SensorExcerptArray::new()
    }
    .data_source_uri(&temperature.odata_id)
    .device_name(&temperature.name);
    let internal = SensorExcerpt {
        reading: temperature.reading,
        ..SensorExcerpt::new()
    }
    .data_source_uri(&temperature.odata_id);

    ThermalMetrics::new(
        format!("{CHASSIS_URI}/ThermalSubsystem/ThermalMetrics"),
        "ThermalMetrics",
        "Thermal Metrics",
    )
    .temperature_readings_celsius([reading])
    .temperature_summary_celsius(TemperatureSummary::new().internal(internal))
}

fn thermal_subsystem_response(metrics: &ThermalMetrics) -> ThermalSubsystem {
    ThermalSubsystem::new(
        format!("{CHASSIS_URI}/ThermalSubsystem"),
        "ThermalSubsystem",
        "Thermal Subsystem",
    )
    .thermal_metrics(&metrics.odata_id)
    .status(enabled_status())
}

#[derive(serde::Serialize)]
struct Responses {
    #[serde(rename = "ServiceRoot")]
    service_root: ServiceRoot,
    #[serde(rename = "ChassisCollection")]
    chassis_collection: ChassisCollection,
    #[serde(rename = "Chassis")]
    chassis: Chassis,
    #[serde(rename = "SensorCollection")]
    sensor_collection: SensorCollection,
    #[serde(rename = "Sensors")]
    sensors: [Sensor; 2],
    #[serde(rename = "ThermalSubsystem")]
    thermal_subsystem: ThermalSubsystem,
    #[serde(rename = "ThermalMetrics")]
    thermal_metrics: ThermalMetrics,
}

fn responses() -> Result<Responses, Box<dyn std::error::Error>> {
    let sensors = sensor_responses()?;
    let sensor_collection = sensor_collection_response(&sensors);
    let [temperature, _] = &sensors;
    let thermal_metrics = thermal_metrics_response(temperature);
    let thermal_subsystem = thermal_subsystem_response(&thermal_metrics);
    let chassis = chassis_response(&sensor_collection, &thermal_subsystem);
    let chassis_collection = chassis_collection_response(&chassis);
    let service_root = service_root_response(&chassis_collection)?;
    Ok(Responses {
        service_root,
        chassis_collection,
        chassis,
        sensor_collection,
        sensors,
        thermal_subsystem,
        thermal_metrics,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let responses = responses()?;
    let mut stdout = io::stdout().lock();
    serde_json::to_writer_pretty(&mut stdout, &responses)?;
    writeln!(stdout)?;
    Ok(())
}
