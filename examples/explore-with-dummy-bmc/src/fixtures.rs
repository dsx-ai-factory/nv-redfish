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

use std::convert::Infallible;

use nv_redfish_bmc_mock::{Bmc, Expect};

const PCIE_FUNCTION_COLLECTION_URI: &str = "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions";
const PCIE_FUNCTION_URI: &str = "/redfish/v1/Chassis/1/PCIeDevices/0-24/PCIeFunctions/0-24-0";
const CONTOSO_SERVICE_URI: &str = "/redfish/v1/Oem/Contoso/TurboencabulatorService";
const ACCOUNT_COLLECTION_URI: &str = "/redfish/v1/AccountService/Accounts";

const SERVICE_ROOT: &str = r##"{
    "@odata.id": "/redfish/v1",
    "@odata.type": "dummy type",
    "Id": "RootService",
    "Name": "Root Service",
    "RedfishVersion": "1.19.0",
    "UUID": "12345678-1234-1234-1234-123456789012",
    "AccountService": {
        "@odata.id": "/redfish/v1/AccountService"
    },
    "Chassis": {
        "@odata.id": "/redfish/v1/Chassis"
    },
    "Systems": {
        "@odata.id": "/redfish/v1/Systems"
    },
    "SessionService": {
        "@odata.id": "/redfish/v1/SessionService"
    },
    "Managers": {
        "@odata.id": "/redfish/v1/Managers"
    },
    "Tasks": {
        "@odata.id": "/redfish/v1/TaskService"
    },
    "EventService": {
        "@odata.id": "/redfish/v1/EventService"
    },
    "Registries": {
        "@odata.id": "/redfish/v1/Registries"
    },
    "JsonSchemas": {
        "@odata.id": "/redfish/v1/JsonSchemas"
    },
    "Links": {
        "Sessions": {
            "@odata.id": "/redfish/v1/SessionService/Sessions"
        }
    },
    "Oem": {
        "Contoso": {
            "@odata.type": "#ContosoServiceRoot.v1_0_0.ServiceRoot",
            "TurboencabulatorService": {
                "@odata.id": "/redfish/v1/Oem/Contoso/TurboencabulatorService"
            }
        }
    }
}"##;

const CHASSIS_COLLECTION: &str = r##"{
    "@odata.id": "/redfish/v1/Chassis",
    "@odata.type": "dummy type",
    "Name": "Chassis Collection",
    "Members": [
        {
            "@odata.id": "/redfish/v1/Chassis/1"
        }
    ]
}"##;

const CHASSIS: &str = r##"{
    "@odata.id": "/redfish/v1/Chassis/1",
    "@odata.type": "dummy type",
    "Id": "1",
    "Name": "Chassis 1",
    "ChassisType": "Rack",
    "Manufacturer": "NVIDIA",
    "Model": "DGX-H100",
    "SerialNumber": "ABC123-1",
    "PCIeDevices": {
        "@odata.id": "/redfish/v1/Chassis/1/PCIeDevices"
    },
    "Status": {
        "State": "Enabled",
        "Health": "OK"
    }
}"##;

const PCIE_DEVICE_COLLECTION: &str = r##"{
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
}"##;

const PCIE_DEVICE_0_24: &str = r##"{
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
}"##;

const PCIE_DEVICE_0_25: &str = r##"{
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
}"##;

const PCIE_FUNCTION_COLLECTION: &str = r##"{
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
}"##;

const PCIE_FUNCTION: &str = r##"{
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
}"##;

const SYSTEM_COLLECTION: &str = r##"{
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
}"##;

const SYSTEM: &str = r##"{
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
    "Oem": {},
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
    "Processors": {
        "@odata.id": "/redfish/v1/Systems/1/Processors"
    },
    "SKU": "5D68144",
    "SecureBoot": {
        "@odata.id": "/redfish/v1/Systems/1/SecureBoot"
    },
    "SerialNumber": "ABC123-1",
    "SimpleStorage": {
        "@odata.id": "/redfish/v1/Systems/1/SimpleStorage"
    },
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
}"##;

const CONTOSO_SERVICE: &str = r##"{
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
}"##;

const ACCOUNT_SERVICE: &str = r##"{
    "@odata.id": "/redfish/v1/AccountService",
    "@odata.type": "dummy type",
    "Id": "AccountService",
    "Accounts": {
        "@odata.id": "/redfish/v1/AccountService/Accounts"
    },
    "Description": "User Accounts",
    "LocalAccountAuth": "Enabled",
    "MinPasswordLength": 8,
    "Name": "Account Service",
    "Oem": {},
    "Roles": {
        "@odata.id": "/redfish/v1/AccountService/Roles"
    }
}"##;

const ACCOUNT_COLLECTION: &str = r##"{
    "@odata.id": "/redfish/v1/AccountService/Accounts",
    "@odata.type": "dummy type",
    "Description": "User Accounts",
    "Name": "Accounts",
    "Members": []
}"##;

const ACCOUNT: &str = r##"{
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
}"##;

const SSE_EVENTS: &str = r##"[
    {
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
    },
    {
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
    }
]"##;

/// Queue the mock requests in the order the walkthrough makes them.
pub(super) fn scripted_bmc() -> Bmc<Infallible> {
    let bmc = Bmc::default();

    let expectations = [
        Expect::get("/redfish/v1", SERVICE_ROOT),
        Expect::get("/redfish/v1/Chassis", CHASSIS_COLLECTION),
        Expect::get("/redfish/v1/Chassis/1", CHASSIS),
        Expect::get("/redfish/v1/Chassis/1/PCIeDevices", PCIE_DEVICE_COLLECTION),
        Expect::get("/redfish/v1/Chassis/1/PCIeDevices/0-24", PCIE_DEVICE_0_24),
        Expect::get(PCIE_FUNCTION_COLLECTION_URI, PCIE_FUNCTION_COLLECTION),
        Expect::get(PCIE_FUNCTION_URI, PCIE_FUNCTION),
        // Both PCIe devices link to the same function collection.
        Expect::get("/redfish/v1/Chassis/1/PCIeDevices/0-25", PCIE_DEVICE_0_25),
        Expect::get(PCIE_FUNCTION_COLLECTION_URI, PCIE_FUNCTION_COLLECTION),
        Expect::get(PCIE_FUNCTION_URI, PCIE_FUNCTION),
        Expect::get("/redfish/v1/Systems", SYSTEM_COLLECTION),
        Expect::get("/redfish/v1/Systems/1", SYSTEM),
        Expect::action(
            "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
            r#"{"ResetType":"ForceRestart"}"#,
            "null",
        ),
        Expect::get(CONTOSO_SERVICE_URI, CONTOSO_SERVICE),
        Expect::update(
            CONTOSO_SERVICE_URI,
            r#"{"TurboencabulatorMode":"Turbo"}"#,
            CONTOSO_SERVICE,
        ),
        Expect::get(CONTOSO_SERVICE_URI, CONTOSO_SERVICE),
        Expect::get("/redfish/v1/AccountService", ACCOUNT_SERVICE),
        Expect::get(ACCOUNT_COLLECTION_URI, ACCOUNT_COLLECTION),
        Expect::create(
            ACCOUNT_COLLECTION_URI,
            r#"{"Password":"secret_password","UserName":"Administrator","RoleId":"admin"}"#,
            ACCOUNT,
        ),
        Expect::stream("/redfish/v1/EventService/SSE", SSE_EVENTS),
    ];

    for expectation in expectations {
        bmc.expect(expectation);
    }

    bmc
}
