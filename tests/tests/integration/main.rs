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
//! Integration tests, built as one binary: each separate test binary
//! instantiates nv-redfish's generic code again.

mod account_service;
mod assembly_wiwynn_missing_odata_type;
mod base_operations;
mod bios;
mod chassis;
mod chassis_oem_delta_power_supply;
mod chassis_oem_liteon_power_supply;
mod chassis_oem_nvidia;
mod compat;
mod compat_http;
mod component_integrity;
mod computer_system;
mod computer_system_oem_hpe;
mod computer_system_oem_lenovo;
mod computer_system_oem_nvidia;
mod computer_system_oem_supermicro;
mod create;
mod event_service_repairs;
mod event_service_resume;
mod event_service_sse_wiwynn;
mod event_service_subscriptions;
mod fabric;
mod inheritance;
mod job_service;
mod manager;
mod manager_network_protocol_oem_hpe;
mod manager_oem_ami;
mod manager_oem_dell;
mod manager_oem_dell_attributes;
mod manager_oem_hpe;
mod manager_oem_lenovo;
mod manager_oem_supermicro;
mod metrics_oem_nvidia;
mod network_adapter_port;
mod oem_nvidia_dpu;
mod power_equipment;
mod processor_oem_nvidia;
mod secure_boot_databases;
mod service_root_oem_ami;
mod service_root_oem_hpe;
mod session_service;
mod storage_oem_dell;
mod task_poll;
mod task_service;
mod telemetry_service;
mod update_service;
mod update_service_oem_ami;
mod update_service_oem_nvidia;
mod vera_rubin;
