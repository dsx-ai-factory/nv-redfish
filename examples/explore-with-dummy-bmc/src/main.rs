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

use std::error::Error as StdError;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::sync::Arc;

use futures_util::StreamExt;
use nv_redfish_core::query::ExpandQuery;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::HttpPushUriUpdateRequest;
use nv_redfish_core::{
    Action, ActionError, Bmc, EntityTypeRef, Expandable, ModificationResponse,
    MultipartUpdateRequest, NavProperty, ODataETag, ODataId, SessionCreateResponse, Updatable,
    UploadReader,
};
use redfish_oem_contoso::redfish::contoso_turboencabulator_service::{
    ContosoTurboencabulatorServiceUpdate, TurboencabulatorMode,
};
use redfish_std::redfish::manager_account::ManagerAccount;
use redfish_std::redfish::manager_account::ManagerAccountCreate;
use redfish_std::redfish::resource::ResetType;
use redfish_std::redfish::service_root::ServiceRoot;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug)]
pub enum Error {
    GenericError,
    NotFound,
    NetworkError,
    AuthError,
    NotSupported,
    CannotFillOem(serde_json::Error),
    ParseError(serde_json::Error),
    ExpectedField(&'static str),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::GenericError => write!(f, "generic error"),
            Self::NotFound => write!(f, "not found"),
            Self::NetworkError => write!(f, "network error"),
            Self::AuthError => write!(f, "auth error"),
            Self::NotSupported => write!(f, "not supported"),
            Self::CannotFillOem(err) => write!(f, "cannot fill OEM: {err}"),
            Self::ParseError(err) => write!(f, "parse error: {err}"),
            Self::ExpectedField(field) => write!(f, "field is absent: {field}"),
        }
    }
}

impl StdError for Error {}

#[derive(Debug, Default)]
pub struct MockBmc {}

impl MockBmc {
    pub async fn get_service_root(&self) -> Result<Arc<ServiceRoot>, Error> {
        NavProperty::<ServiceRoot>::new_reference(ODataId::service_root())
            .get(self)
            .await
    }

    fn get_mock_json_for_uri(&self, uri: &str) -> String {
        match uri {
            "/redfish/v1" => {
                r##"{
                      "@odata.id": "/redfish/v1",
                      "@odata.type": "dummy type",
                      "Id": "RootService",
                      "Name": "Root Service",
                      "RedfishVersion": "1.19.0",
                      "UUID": "12345678-1234-1234-1234-123456789012",
                      "AccountService": { "@odata.id": "/redfish/v1/AccountService" },
                      "Chassis": {"@odata.id": "/redfish/v1/Chassis"},
                      "Systems": {"@odata.id": "/redfish/v1/Systems"},
                      "SessionService": {"@odata.id": "/redfish/v1/SessionService"},
                      "Managers": {"@odata.id": "/redfish/v1/Managers"},
                      "Tasks": { "@odata.id": "/redfish/v1/TaskService"},
                      "EventService": {"@odata.id": "/redfish/v1/EventService"},
                      "Registries": {"@odata.id": "/redfish/v1/Registries"},
                      "JsonSchemas": {"@odata.id": "/redfish/v1/JsonSchemas"},
                      "Links": {
                         "Sessions": {"@odata.id": "/redfish/v1/SessionService/Sessions"}
                      },
                      "Oem": {
                          "Contoso": {
                              "@odata.type": "#ContosoServiceRoot.v1_0_0.ServiceRoot",
                              "TurboencabulatorService": {
                                  "@odata.id": "/redfish/v1/Oem/Contoso/TurboencabulatorService"
                              }
                          }
                      }
                   }"##.to_string()
            },
            "/redfish/v1/Chassis" => {
                r#"{
                      "@odata.id": "/redfish/v1/Chassis",
                      "@odata.type": "dummy type",
                      "Name": "Chassis Collection",
                      "Members": [
                          {
                              "@odata.id": "/redfish/v1/Chassis/1"
                          }
                      ]
                }"#.to_string()
            },
            "/redfish/v1/Chassis/1" => {
                r#"{
                   "@odata.id": "/redfish/v1/Chassis/1",
                   "@odata.type": "dummy type",
                   "Id": "1",
                   "Name": "Chassis 1",
                   "ChassisType": "Rack",
                   "Manufacturer": "NVIDIA",
                   "Model": "DGX-H100",
                   "SerialNumber": "ABC123-1",
                   "PCIeDevices": { "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices" },
                   "Status": {"State": "Enabled", "Health": "OK"}
                }"#.to_string()
            },
            "/redfish/v1/Chassis/1/PCIeDevices" => {
                r#"{
                      "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices",
                      "@odata.type": "dummy type",
                      "Name": "Chassis Collection",
                      "Members": [
                          {
                              "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24"
                          },
                          {
                              "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-25"
                          }
                      ]
                }"#.to_string()
            },
            "/redfish/v1/Chassis/1/PCIeDevices/0-24" => {
                r#"{
                     "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24",
                     "@odata.type": "dummy type",
                     "Id": "0-24",
                     "Links": {},
                     "Manufacturer": "Intel Corporation",
                     "Model": null,
                     "Name": "Sapphire Rapids SATA AHCI Controller",
                     "PartNumber": null,
                     "SKU": null,
                     "SerialNumber": null,
                     "Slot": {},
                     "Status": {
                         "State": "Enabled",
                         "Health": "OK",
                         "HealthRollup": "OK"
                     },
                     "PCIeFunctions": {
                         "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions"
                     }
                }"#.to_string()
            },
            "/redfish/v1/Chassis/1/PCIeDevices/0-25" => {
                r##"{
                    "@odata.context": "/redfish/v1/$metadata#PCIeDevice.PCIeDevice",
                    "@odata.etag": "\"1754525527\"",
                    "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-25",
                    "@odata.type": "#PCIeDevice.v1_14_0.PCIeDevice",
                    "AssetTag": null,
                    "Description": "Sapphire Rapids SATA AHCI Controller",
                    "DeviceType": "SingleFunction",
                    "FirmwareVersion": "",
                    "Id": "0-25",
                    "Links": {},
                    "Manufacturer": "Intel Corporation",
                    "Model": null,
                    "Name": "Sapphire Rapids SATA AHCI Controller",
                    "PartNumber": null,
                    "SKU": null,
                    "SerialNumber": null,
                    "Slot": {},
                    "Status": {
                        "State": "Enabled",
                        "Health": "OK",
                        "HealthRollup": "OK"
                    },
                    "PCIeFunctions": {
                        "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions"
                    }
                }"##.to_string()
            },
            "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions" => {
                r#"{
                    "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions",
                    "@odata.type": "dummy type",
                    "Description": "Collection of PCIeFunctions",
                    "Members": [
                        {
                            "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions/0-24-0"
                        }
                    ],
                    "Members@odata.count": 1,
                    "Name": "PCIeFunction Collection"
                }"#.to_string()
            },
            "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions/0-24-0" => {
                r#"
                {
                    "@odata.context": "/redfish/v1/$metadata#PCIeFunction.PCIeFunction",
                    "@odata.etag": "\"1754525529\"",
                    "@odata.type": "dummy type",
                    "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions/0-24-0",
                    "ClassCode": "0x010601",
                    "Description": "Sapphire Rapids SATA AHCI Controller",
                    "DeviceClass": "MassStorageController",
                    "DeviceId": "0x1bf2",
                    "Enabled": true,
                    "FunctionId": 0,
                    "FunctionType": "Physical",
                    "Id": "0-24-0",
                    "Links": {},
                    "Name": "Sapphire Rapids SATA AHCI Controller",
                    "RevisionId": "0x11",
                    "Status": {
                        "State": "Enabled",
                        "Health": "OK",
                        "HealthRollup": "OK"
                    },
                    "SubsystemId": "0x0a6b",
                    "SubsystemVendorId": "0x1028",
                    "VendorId": "0x8086"
                }"#.to_string()
            },
            "/redfish/v1/Systems" => {
                r#"{
                    "@odata.context": "/redfish/v1/$metadata#ComputerSystemCollection.ComputerSystemCollection",
                    "@odata.id": "/redfish/v1/Systems",
                    "@odata.type": "dummy type",
                    "Description": "Collection of Computer Systems",
                    "Members": [
                        {
                            "@odata.id": "/redfish/v1/Systems/1"
                        }
                    ],
                    "Members@odata.count": 1,
                    "Name": "Computer System Collection"
                }"#.to_string()
            },
            "/redfish/v1/Systems/1" => {
                r##"{
                    "@Redfish.Settings": {
                        "@odata.context": "/redfish/v1/$metadata#Settings.Settings",
                        "SettingsObject": {
                            "@odata.id": "/redfish/v1/Systems/1/Settings"
                        },
                        "SupportedApplyTimes": [
                            "OnReset"
                        ]
                    },
                    "@odata.context": "/redfish/v1/$metadata#ComputerSystem.ComputerSystem",
                    "@odata.id": "/redfish/v1/Systems/1",
                    "@odata.type": "dummy type",
                    "Actions": {
                        "#ComputerSystem.Reset": {
                            "target": "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
                            "ResetType@Redfish.AllowableValues": [
                                "On",
                                "ForceOff",
                                "ForceRestart",
                                "GracefulRestart",
                                "GracefulShutdown",
                                "PushPowerButton",
                                "Nmi",
                                "PowerCycle"
                            ]
                        }
                    },
                    "AssetTag": "",
                    "Bios": {
                        "@odata.id": "/redfish/v1/Systems/1/Bios"
                    },
                    "BiosVersion": "2.5.4",
                    "BootProgress": {
                        "LastState": "OSRunning"
                    },
                    "Boot": {
                        "BootOptions": {
                            "@odata.id": "/redfish/v1/Systems/1/BootOptions"
                        },
                        "Certificates": {
                            "@odata.id": "/redfish/v1/Systems/1/Boot/Certificates"
                        },
                        "BootOrder": [
                            "Boot0000",
                            "Boot0001"
                        ],
                        "BootOrder@odata.count": 2,
                        "BootSourceOverrideEnabled": "Disabled",
                        "BootSourceOverrideMode": "UEFI",
                        "BootSourceOverrideTarget": "None",
                        "UefiTargetBootSourceOverride": null,
                        "BootSourceOverrideTarget@Redfish.AllowableValues": [
                            "None",
                            "Pxe",
                            "Floppy",
                            "Cd",
                            "Hdd",
                            "BiosSetup",
                            "Utilities",
                            "UefiTarget",
                            "SDCard",
                            "UefiHttp"
                        ],
                        "StopBootOnFault": "Never"
                    },
                    "Description": "Computer System which represents a machine (physical or virtual) and the local resources such as memory, cpu and other devices that can be accessed from that machine.",
                    "EthernetInterfaces": {
                        "@odata.id": "/redfish/v1/Systems/1/EthernetInterfaces"
                    },
                    "GraphicalConsole": {
                        "ConnectTypesSupported": [
                            "KVMIP"
                        ],
                        "ConnectTypesSupported@odata.count": 1,
                        "MaxConcurrentSessions": 6,
                        "ServiceEnabled": true
                    },
                    "HostName": "",
                    "HostWatchdogTimer": {
                        "FunctionEnabled": false,
                        "Status": {
                            "State": "Disabled"
                        },
                        "TimeoutAction": "None"
                    },
                    "HostingRoles": [],
                    "HostingRoles@odata.count": 0,
                    "Id": "1",
                    "IndicatorLED": "Lit",
                    "IndicatorLED@Redfish.Deprecated": "Please migrate to use LocationIndicatorActive property",
                    "Links": {},
                    "LastResetTime": "2025-06-16T19:47:38-05:00",
                    "LocationIndicatorActive": false,
                    "Manufacturer": "Dell Inc.",
                    "Memory": {
                        "@odata.id": "/redfish/v1/Systems/1/Memory"
                    },
                    "MemorySummary": {
                        "MemoryMirroring": "System",
                        "Status": {
                            "Health": "OK",
                            "HealthRollup": "OK",
                            "State": "Enabled"
                        },
                        "Status@Redfish.Deprecated": "Please migrate to use Status in the individual Memory resources",
                        "TotalSystemMemoryGiB": 256
                    },
                    "Model": "PowerEdge R760",
                    "Name": "System",
                    "NetworkInterfaces": {
                        "@odata.id": "/redfish/v1/Systems/1/NetworkInterfaces"
                    },
                    "Oem": {
                    },
                    "PCIeDevices": [],
                    "PCIeDevices@odata.count": 9,
                    "PCIeFunctions": [],
                    "PCIeFunctions@odata.count": 12,
                    "PartNumber": "ABC123-1-1",
                    "PowerState": "On",
                    "ProcessorSummary": {
                        "Count": 2,
                        "CoreCount": 48,
                        "LogicalProcessorCount": 96,
                        "Model": "Intel(R) Xeon(R) Gold 6442Y",
                        "Status": {
                            "Health": "OK",
                            "HealthRollup": "OK",
                            "State": "Enabled"
                        },
                        "Status@Redfish.Deprecated": "Please migrate to use Status in the individual Processor resources",
                        "ThreadingEnabled": true
                    },
                    "Processors": { "@odata.id": "/redfish/v1/Systems/1/Processors" },
                    "SKU": "5D68144",
                    "SecureBoot": { "@odata.id": "/redfish/v1/Systems/1/SecureBoot" },
                    "SerialNumber": "ABC123-1",
                    "SimpleStorage": { "@odata.id": "/redfish/v1/Systems/1/SimpleStorage" },
                    "Status": {
                        "Health": "OK",
                        "HealthRollup": "OK",
                        "State": "Enabled"
                    },
                    "Storage": {
                        "@odata.id": "/redfish/v1/Systems/1/Storage"
                    },
                    "SystemType": "Physical",
                    "TrustedModules": [
                        {
                            "FirmwareVersion": "7.2.3.1",
                            "InterfaceType": "TPM2_0",
                            "Status": {
                                "State": "Enabled"
                            }
                        }
                    ],
                    "TrustedModules@odata.count": 1,
                    "UUID": "4c4c4544-0044-3610-8038-b5c04f313434",
                    "VirtualMedia": {
                        "@odata.id": "/redfish/v1/Systems/1/VirtualMedia"
                    },
                    "VirtualMediaConfig": {
                        "ServiceEnabled": true
                    }
                }"##.to_string()
            }
            "/redfish/v1/Oem/Contoso/TurboencabulatorService" => {
                r##"{
                       "@odata.id": "/redfish/v1/Oem/Contoso/TurboencabulatorService",
                       "@odata.type": "dummy type",
                       "Id": "TurboencabulatorService",
                       "Name": "Turboencabulator Service",
                       "Status": {
                           "State": "Enabled",
                           "Health": "OK"
                       },
                       "IsCheap": false,
                       "ServiceEnabled": true,
                       "TurboencabulatorMode": "Retro",
                       "WillGovernmentBuy": true
                   }"##.to_string()
            }
            "/redfish/v1/AccountService" => {
                r##"{
                   "@odata.id": "/redfish/v1/AccountService",
                   "@odata.type": "dummy type",
                   "Id": "AccountService",
                   "Accounts": { "@odata.id": "/redfish/v1/AccountService/Accounts" },
                   "Description": "User Accounts",
                   "LocalAccountAuth": "Enabled",
                   "MinPasswordLength": 8,
                   "Name": "Account Service",
                   "Oem": {},
                   "Roles": { "@odata.id": "/redfish/v1/AccountService/Roles" }
               }"##.to_string()
            }
            "/redfish/v1/AccountService/Accounts" => {
               r##"{
                   "@odata.id": "/redfish/v1/AccountService/Accounts",
                   "@odata.type": "dummy type",
                   "Description": "User Accounts",
                   "Name": "Accounts",
                   "Members": []
               }"##.to_string()
            }
            "/redfish/v1/AccountService/Accounts/1" => {
                r##"{
                   "@odata.id": "/redfish/v1/AccountService/Accounts/1",
                   "@odata.type": "dummy type",
                   "AccountTypes": [],
                   "Id": "1",
                   "Description": "User Account",
                   "Enabled": true,
                   "Links": {},
                   "Name": "User Account",
                   "Oem": {},
                   "Password": null,
                   "PasswordChangeRequired": false,
                   "RoleId": "Administrator",
                   "UserName": "Administrator"
               }"##.to_string()
            }
            _ => {
                r#"{"id": "unknown", "name": "Unknown Resource"}"#.to_string()
            }
        }
    }
}

impl Bmc for MockBmc {
    type Error = Error;

    async fn expand<T>(&self, _id: &ODataId, _query: ExpandQuery) -> Result<Arc<T>, Error>
    where
        T: Expandable,
    {
        todo!("unimplimented")
    }

    async fn filter<T: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        _id: &ODataId,
        _query: nv_redfish_core::FilterQuery,
    ) -> Result<Arc<T>, Error> {
        todo!("unimplimented")
    }

    async fn get<T: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        id: &ODataId,
    ) -> Result<Arc<T>, Self::Error> {
        // println!("BMC GET {id}");
        // In real implementation: async HTTP GET request and JSON deserialization
        let mock_json = self.get_mock_json_for_uri(&id.to_string());
        let result: T = serde_json::from_str(&mock_json).map_err(Error::ParseError)?;
        Ok(Arc::new(result))
    }

    async fn update<
        V: Sync + Send + Serialize,
        R: Sync + Send + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        _etag: Option<&ODataETag>,
        update: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        println!(
            "BMC Update {}: {}",
            id,
            serde_json::to_string(update).expect("serializable")
        );
        let mock_json = self.get_mock_json_for_uri(&id.to_string());
        let result: R = serde_json::from_str(&mock_json).map_err(Error::ParseError)?;
        Ok(ModificationResponse::Entity(result))
    }

    async fn create<
        V: Sync + Send + Serialize,
        R: Sync + Send + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        create: &V,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        println!(
            "BMC create {}: {}",
            id,
            serde_json::to_string(create).expect("serializable")
        );
        let mock_json = self.get_mock_json_for_uri("/redfish/v1/AccountService/Accounts/1");
        let result: R = serde_json::from_str(&mock_json).map_err(Error::ParseError)?;
        Ok(ModificationResponse::Entity(result))
    }

    async fn create_session<
        V: Sync + Send + Serialize,
        R: Sync + Send + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        id: &ODataId,
        create: &V,
    ) -> Result<SessionCreateResponse<R>, Self::Error> {
        println!(
            "BMC create session {}: {}",
            id,
            serde_json::to_string(create).expect("serializable")
        );
        let session_id = "/redfish/v1/SessionService/Sessions/1";
        let mock_json = self.get_mock_json_for_uri(session_id);
        let entity: R = serde_json::from_str(&mock_json).map_err(Error::ParseError)?;
        Ok(SessionCreateResponse {
            entity,
            auth_token: "dummy-session-token".to_string(),
            location: session_id.to_string().into(),
        })
    }

    async fn delete<R: EntityTypeRef + for<'de> Deserialize<'de>>(
        &self,
        _id: &ODataId,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        Ok(ModificationResponse::Empty)
    }

    async fn action<
        T: Send + Sync + serde::Serialize,
        R: Send + Sync + Sized + for<'de> Deserialize<'de>,
    >(
        &self,
        _action: &Action<T, R>,
        _params: &T,
    ) -> Result<ModificationResponse<R>, Self::Error> {
        let result: R = serde_json::from_str("null").map_err(Error::ParseError)?;
        Ok(ModificationResponse::Entity(result))
    }

    async fn multipart_update<U, V, R>(
        &self,
        _uri: &str,
        _request: MultipartUpdateRequest<'_, U, V>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
        V: Send + Sync + Serialize,
    {
        Err(Error::NotSupported)
    }

    #[cfg(feature = "update-service-deprecated")]
    async fn http_push_uri_update<U, R>(
        &self,
        _uri: &str,
        _request: HttpPushUriUpdateRequest<U>,
    ) -> Result<ModificationResponse<R>, Self::Error>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> Deserialize<'de>,
    {
        Err(Error::NotSupported)
    }

    async fn stream<T: Send + Sized + for<'de> Deserialize<'de> + 'static>(
        &self,
        _id: &str,
    ) -> Result<nv_redfish_core::BoxTryStream<T, Self::Error>, Self::Error> {
        let payloads = vec![
            serde_json::json!({
                "@odata.type": "#Event.v1_6_0.Event",
                "Id": "1",
                "Name": "Event Array",
                "Context": "ABCDEFGH",
                "Events": [
                    {
                        "@odata.id": "/redfish/v1/SomeService",
                        "@odata.type": "#Event.v1_0_0.EventRecord",
                        "MemberId": "1",
                        "EventType": "Alert",
                        "EventId": "1",
                        "Severity": "Warning",
                        "MessageSeverity": "Warning",
                        "Message": "The LAN has been disconnected",
                        "MessageId": "Alert.1.0.LanDisconnect",
                        "MessageArgs": [
                            "EthernetInterface 1",
                            "/redfish/v1/Systems/1"
                        ],
                        "OriginOfCondition": {
                            "@odata.id": "/redfish/v1/Systems/1/EthernetInterfaces/1"
                        },
                        "Context": "ABCDEFGH"
                    }
                ]
            }),
            serde_json::json!({
                "@odata.id": "/redfish/v1/TelemetryService/MetricReports/AvgPlatformPowerUsage",
                "@odata.type": "#MetricReport.v1_3_0.MetricReport",
                "Id": "AvgPlatformPowerUsage",
                "Name": "Average Platform Power Usage metric report",
                "MetricReportDefinition": {
                    "@odata.id": "/redfish/v1/TelemetryService/MetricReportDefinitions/AvgPlatformPowerUsage"
                },
                "MetricValues": [
                    {
                        "MetricId": "AverageConsumedWatts",
                        "MetricValue": "100",
                        "Timestamp": "2016-11-08T12:25:00-05:00",
                        "MetricProperty": "/redfish/v1/Chassis/Tray_1/Power#/0/PowerConsumedWatts"
                    },
                    {
                        "MetricId": "AverageConsumedWatts",
                        "MetricValue": "94",
                        "Timestamp": "2016-11-08T13:25:00-05:00",
                        "MetricProperty": "/redfish/v1/Chassis/Tray_1/Power#/0/PowerConsumedWatts"
                    },
                    {
                        "MetricId": "AverageConsumedWatts",
                        "MetricValue": "100",
                        "Timestamp": "2016-11-08T14:25:00-05:00",
                        "MetricProperty": "/redfish/v1/Chassis/Tray_1/Power#/0/PowerConsumedWatts"
                    }
                ]
            }),
        ];

        let events: Vec<T> = payloads
            .into_iter()
            .map(|event| serde_json::from_value(event).map_err(Error::ParseError))
            .collect::<Result<_, _>>()?;

        Ok(Box::pin(futures_util::stream::iter(
            events.into_iter().map(Ok),
        )))
    }
}

impl ActionError for Error {
    fn not_supported() -> Self {
        Error::NotSupported
    }
}

#[derive(Deserialize, Debug)]
struct Constoso {
    #[serde(rename = "Contoso")]
    oem_root: redfish_oem_contoso::redfish::contoso_service_root::ServiceRoot,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let bmc = MockBmc::default();

    let service_root = bmc.get_service_root().await?;

    let chassis = &service_root
        .chassis
        .as_ref()
        .ok_or(Error::ExpectedField("chassis"))?
        .get(&bmc)
        .await?;

    println!("Discoveried chassis:");
    for m in &chassis.members {
        println!("  {}", m.id());
    }

    for chassis in &chassis.members {
        let chassis = chassis.get(&bmc).await?;
        println!("Chassis: {} (id: {})", chassis.name, chassis.id);
        println!(
            "  Model: {}",
            chassis
                .model
                .as_ref()
                .and_then(Option::as_ref)
                .unwrap_or(&"unknown".to_string())
        );
        let pcie_devices = chassis
            .pcie_devices
            .as_ref()
            .ok_or(Error::ExpectedField("pcie_devices"))?
            .get(&bmc)
            .await?;
        for pcie_device in &pcie_devices.members {
            let pcie_device = pcie_device.get(&bmc).await?;
            println!(
                "  PCI Device: {} (id: {})",
                pcie_device.name, pcie_device.id
            );
            let pcie_functions = pcie_device
                .pcie_functions
                .as_ref()
                .ok_or(Error::ExpectedField("pcie_functions"))?
                .get(&bmc)
                .await?;
            for pcie_function in &pcie_functions.members {
                let pcie_function = pcie_function.get(&bmc).await?;
                println!(
                    "    Function: {} (id: {})",
                    pcie_function.name, pcie_function.id
                );
            }
        }
    }

    let systems = &service_root
        .systems
        .as_ref()
        .ok_or(Error::ExpectedField("systems"))?
        .get(&bmc)
        .await?;
    let system = systems
        .members
        .first()
        .ok_or(Error::ExpectedField("first system"))?
        .get(&bmc)
        .await?;

    println!("System {} (id: {}):", system.name, system.id);
    println!(
        "  BIOS Version: {}",
        system
            .bios_version
            .as_ref()
            .and_then(Option::as_ref)
            .unwrap_or(&"unknown".to_string())
    );

    println!("Performing system reset...");
    let _reset = system
        .actions
        .as_ref()
        .ok_or(Error::ExpectedField("actions"))?
        .reset(&bmc, Some(ResetType::ForceRestart))
        .await?;
    println!("  Ok!");

    println!("Browse OEM extension:");
    // Oem:
    let contoso_oem: Constoso = serde_json::from_value(
        service_root
            .oem
            .as_ref()
            .ok_or(Error::ExpectedField("oem"))?
            .additional_properties
            .clone(),
    )
    .map_err(Error::CannotFillOem)?;

    let turboencabulator_service = contoso_oem
        .oem_root
        .turboencabulator_service
        .ok_or(Error::ExpectedField("turboencabulator_service"))?
        .get(&bmc)
        .await?;

    println!("  Turboencabulator service:");
    println!(
        "    service enabled: {}",
        turboencabulator_service
            .service_enabled
            .as_ref()
            .and_then(Option::as_ref)
            .map(ToString::to_string)
            .unwrap_or("unknown".to_string())
    );
    println!(
        "    will government buy: {}",
        turboencabulator_service
            .will_government_buy
            .as_ref()
            .and_then(Option::as_ref)
            .map(ToString::to_string)
            .unwrap_or("unknown".to_string())
    );
    println!(
        "    mode: {:?}",
        turboencabulator_service.turboencabulator_mode
    );

    let update = ContosoTurboencabulatorServiceUpdate::builder()
        .with_turboencabulator_mode(TurboencabulatorMode::Turbo);

    let updated = turboencabulator_service.update(&bmc, &update).await?;
    let updated = match updated {
        ModificationResponse::Entity(updated) => updated,
        _ => return Err(Error::ExpectedField("typed update payload")),
    };
    let _ = updated.refresh(&bmc).await?;

    println!("Create account");
    let accounts = service_root
        .account_service
        .as_ref()
        .ok_or(Error::ExpectedField("account_service"))?
        .get(&bmc)
        .await?
        .accounts
        .as_ref()
        .ok_or(Error::ExpectedField("accounts"))?
        .get(&bmc)
        .await?;
    let account = bmc
        .create::<_, ManagerAccount>(
            accounts.odata_id(),
            &ManagerAccountCreate::builder(
                "secret_password".into(),
                "Administrator".into(),
                "admin".into(),
            )
            .build(),
        )
        .await?;
    println!("  Ok!");
    println!("Returned account:");
    let account = match account {
        ModificationResponse::Entity(account) => account,
        _ => return Err(Error::ExpectedField("typed account payload")),
    };
    println!(
        "  User name: {}",
        account.user_name.as_ref().unwrap_or(&"-".to_string())
    );
    println!(
        "  Enabled: {}",
        account
            .enabled
            .map(|b| b.to_string())
            .unwrap_or("-".to_string())
    );

    println!("Read mock SSE stream:");
    let mut event_stream = bmc.stream::<Value>("/redfish/v1/EventService/SSE").await?;
    while let Some(event) = event_stream.next().await {
        let event = event?;
        println!("  {:?}", event);
    }

    Ok(())
}
