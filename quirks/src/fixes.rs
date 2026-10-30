// SPDX-FileCopyrightText: Copyright (c) 2025-2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
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

//! The rewrites the repair table composes. Each takes one document and
//! returns it repaired; a document without the fault comes back unchanged.

use nv_redfish_core::{EdmDateTimeOffset, EdmGuid};
use serde_json::json;
use serde_json::Value;

pub fn add_default_update_service_name(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        obj.entry("Name")
            .or_insert(Value::String("Unnamed update service".into()));
        Value::Object(obj)
    } else {
        v
    }
}

// `ReleaseDate` is an `Edm.DateTimeOffset`, but some systems put
// `00:00:00Z` there, which does not conform to its ABNF; the field is
// removed.
pub fn remove_wrong_release_date(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        if let Some(Value::String(date)) = obj.get("ReleaseDate") {
            if date == "00:00:00Z" || date == "0000-00-00T00:00:00Z" {
                obj.remove("ReleaseDate");
            }
        }
        Value::Object(obj)
    } else {
        v
    }
}

pub fn remove_invalid_contained_by_fields(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        if let Some(Value::Object(ref mut links_obj)) = obj.get_mut("Links") {
            if let Some(Value::Object(ref mut contained_by_obj)) = links_obj.get_mut("ContainedBy")
            {
                contained_by_obj.retain(|k, _| k == "@odata.id");
            }
        }
    }
    v
}

pub fn certificates_as_reference(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        if let Some(Value::Object(ref mut certificates)) = obj.get_mut("Certificates") {
            certificates.retain(|k, _| k == "@odata.id");
        }
    }
    v
}

pub fn add_default_chassis_type(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        obj.entry("ChassisType")
            .or_insert(Value::String("Other".into()));
        Value::Object(obj)
    } else {
        v
    }
}

pub fn add_default_chassis_name(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        obj.entry("Name")
            .or_insert(Value::String("Unnamed chassis".into()));
        Value::Object(obj)
    } else {
        v
    }
}

pub fn normalize_empty_uuid_field(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        if let Some(uuid) = obj.get_mut("UUID") {
            let is_empty = uuid.as_str().is_some_and(str::is_empty);
            if is_empty {
                *uuid = Value::Null;
            }
        }
    }
    v
}

// `UUID` is optional, so a value that does not parse as one, an empty
// string or an unresolved template, is read as absent.
pub fn normalize_invalid_uuid_field(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        if let Some(uuid) = obj.get_mut("UUID") {
            let is_invalid = uuid
                .as_str()
                .is_some_and(|s| EdmGuid::parse_str(s).is_err());
            if is_invalid {
                *uuid = Value::Null;
            }
        }
    }
    v
}

// BlueField-4 serves `NvidiaChassis.Reset` on its `BlueField_0` chassis
// without advertising it; the action is advertised at the target the BMC
// serves, and an advertised one is left as it came.
pub fn advertise_bluefield_nvidia_chassis_reset(mut v: Value) -> Value {
    let target = match (
        v.get("Id").and_then(Value::as_str),
        v.get("@odata.id").and_then(Value::as_str),
    ) {
        (Some("BlueField_0"), Some(chassis_id)) => {
            Some(format!("{chassis_id}/Actions/Oem/NvidiaChassis.Reset"))
        }
        _ => None,
    };
    let Some(target) = target else {
        return v;
    };
    if let Some(chassis) = v.as_object_mut() {
        let actions = chassis.entry("Actions").or_insert_with(|| json!({}));
        if let Some(actions) = actions.as_object_mut() {
            let oem = actions.entry("Oem").or_insert_with(|| json!({}));
            if let Some(oem) = oem.as_object_mut() {
                oem.entry("#NvidiaChassis.Reset")
                    .or_insert_with(|| json!({ "target": target }));
            }
        }
    }
    v
}

// `Controllers` is a collection, but iDRAC reports `{}` on an empty
// network adapter slot. A controller list, and an object that is not
// empty, are left as they came.
pub fn normalize_empty_network_adapter_controllers(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        let is_empty_object = obj
            .get("Controllers")
            .is_some_and(|controllers| matches!(controllers, Value::Object(map) if map.is_empty()));
        if is_empty_object {
            obj.insert("Controllers".into(), Value::Array(Vec::new()));
        }
    }
    v
}

// `LastResetTime` is an `Edm.DateTimeOffset`, but some systems put
// `0000-00-00T00:00:00+00:00` there, which does not conform to its ABNF;
// the field is removed.
pub fn remove_wrong_last_reset_time(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        if let Some(Value::String(date)) = obj.get("LastResetTime") {
            if date.starts_with("0000-00-00") {
                obj.remove("LastResetTime");
            }
        }
        Value::Object(obj)
    } else {
        v
    }
}

// `PasswordExpiration` is an `Edm.DateTimeOffset`, which needs seconds, but
// Lenovo XCC 3 leaves them out, as in `2026-10-26T16:49+00:00`; `:00` is
// added.
pub fn add_password_expiration_seconds(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        if let Some(Value::String(date)) = obj.get_mut("PasswordExpiration") {
            if let Some(fixed) = with_seconds(date) {
                *date = fixed;
            }
        }
        Value::Object(obj)
    } else {
        v
    }
}

/// `date` with `:00` seconds when it is `YYYY-MM-DDTHH:MM` and an offset,
/// which is when the result is a valid `Edm.DateTimeOffset`.
fn with_seconds(date: &str) -> Option<String> {
    let (minutes, offset) = date.split_at_checked(16)?;
    let fixed = format!("{minutes}:00{offset}");
    fixed.parse::<EdmDateTimeOffset>().is_ok().then_some(fixed)
}

/// Vera Rubin firmware reports composite `BootOrder` entries such as
/// `"Boot0019: Ubuntu"` while boot option resources use the bare reference.
pub fn normalize_vera_rubin_composite_boot_order(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        if let Some(Value::Object(ref mut boot)) = obj.get_mut("Boot") {
            if let Some(Value::Array(ref mut boot_order)) = boot.get_mut("BootOrder") {
                for entry in boot_order.iter_mut() {
                    if let Value::String(entry) = entry {
                        *entry = vera_rubin_boot_order_entry_reference(entry).to_string();
                    }
                }
            }
        }
    }
    v
}

fn vera_rubin_boot_order_entry_reference(entry: &str) -> &str {
    entry
        .split_once(": ")
        .map_or(entry, |(reference, _)| reference)
}

// `AccountTypes` is `Redfish.Required`, but some systems omit it. The
// schema says: "if this property is not provided by the client, the
// default value shall be an array that contains the value `Redfish`".
pub fn append_default_account_type(v: Value) -> Value {
    if let Value::Object(mut obj) = v {
        obj.entry("AccountTypes")
            .or_insert(Value::Array(vec![Value::String("Redfish".into())]));
        Value::Object(obj)
    } else {
        v
    }
}

/// The non-standard `PEMChain` certificate type, as the H100 Viking
/// spells `PEMchain`.
pub fn normalize_pem_chain_type(mut v: Value) -> Value {
    if v.get("CertificateType").and_then(Value::as_str) == Some("PEMChain") {
        v["CertificateType"] = Value::String("PEMchain".into());
    }
    v
}

/// Certificate collections a DPU or GB host BMC inlines into the AccountService without
/// the `Name` every resource requires become references to themselves, to
/// be read on their own when asked for.
pub fn normalize_dpu_certificate_collection_stubs(mut v: Value) -> Value {
    fn normalize(value: &mut Value) {
        match value {
            Value::Object(object) => {
                let certificate_collection = object
                    .get("@odata.type")
                    .and_then(Value::as_str)
                    .is_some_and(|odata_type| odata_type.starts_with("#CertificateCollection."));
                if certificate_collection && !object.contains_key("Name") {
                    if let Some(odata_id) = object.get("@odata.id").cloned() {
                        object.clear();
                        object.insert("@odata.id".into(), odata_id);
                        return;
                    }
                }
                for value in object.values_mut() {
                    normalize(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    normalize(value);
                }
            }
            _ => {}
        }
    }

    normalize(&mut v);
    v
}

/// The power shelf `Status.State` values that are not a `Resource.State`
/// are read as the nearest one: `Standby` as `StandbySpare`, and `Sleep`,
/// what a forced-off shelf reports, as `StandbyOffline`, which waits for
/// an action to turn it on. Every other state stays.
pub fn normalize_power_shelf_states(mut v: Value) -> Value {
    if let Some(state) = v.pointer_mut("/Status/State") {
        let named = match state.as_str() {
            Some("Standby") => "StandbySpare",
            Some("Sleep") => "StandbyOffline",
            _ => return v,
        };
        *state = Value::String(named.into());
    }
    v
}

/// Lite-On's LED-state object in `PowerSupply.LocationIndicatorActive`,
/// where the schema has a boolean, is removed; schema-valid values stay.
pub fn remove_invalid_power_supply_location_indicator_active(mut v: Value) -> Value {
    if let Value::Object(ref mut obj) = v {
        if obj
            .get("LocationIndicatorActive")
            .is_some_and(Value::is_object)
        {
            obj.remove("LocationIndicatorActive");
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// BlueField-4's `BlueField_0` chassis advertises only the standard
    /// `Chassis.Reset`; the NVIDIA reset it serves is added beside it, and
    /// nothing changes on another chassis or where the action is advertised.
    #[test]
    fn bluefield_chassis_reset_is_advertised_only_where_missing() {
        let target = "/redfish/v1/Chassis/BlueField_0/Actions/Oem/NvidiaChassis.Reset";
        let standard = json!({
            "@Redfish.ActionInfo": "/redfish/v1/Chassis/BlueField_0/ResetActionInfo",
            "target": "/redfish/v1/Chassis/BlueField_0/Actions/Chassis.Reset"
        });
        let chassis = |id: &str, actions: Value| json!({ "@odata.id": format!("/redfish/v1/Chassis/{id}"), "Id": id, "Actions": actions });
        for (scenario, document, expected) in [
            (
                "BlueField_0 as captured",
                chassis("BlueField_0", json!({ "#Chassis.Reset": standard })),
                chassis(
                    "BlueField_0",
                    json!({
                        "#Chassis.Reset": standard,
                        "Oem": { "#NvidiaChassis.Reset": { "target": target } }
                    }),
                ),
            ),
            (
                "another chassis",
                chassis("BlueField_IRoT_NIC_0", json!({})),
                chassis("BlueField_IRoT_NIC_0", json!({})),
            ),
            (
                "advertised",
                chassis(
                    "BlueField_0",
                    json!({ "Oem": { "#NvidiaChassis.Reset": { "target": "/advertised" } } }),
                ),
                chassis(
                    "BlueField_0",
                    json!({ "Oem": { "#NvidiaChassis.Reset": { "target": "/advertised" } } }),
                ),
            ),
        ] {
            assert_eq!(
                advertise_bluefield_nvidia_chassis_reset(document),
                expected,
                "{scenario}"
            );
        }
    }

    #[test]
    fn vera_rubin_boot_order_entry_reference_strips_display_name_suffix() {
        assert_eq!(
            vera_rubin_boot_order_entry_reference("Boot0019: Ubuntu"),
            "Boot0019"
        );
        assert_eq!(
            vera_rubin_boot_order_entry_reference("Boot0010: UEFI HTTPv4 (MAC:AA)"),
            "Boot0010"
        );
        assert_eq!(
            vera_rubin_boot_order_entry_reference("Boot0010"),
            "Boot0010"
        );
    }

    #[test]
    fn normalize_vera_rubin_composite_boot_order_patches_boot_order_array() {
        let patched = normalize_vera_rubin_composite_boot_order(json!({
            "Boot": {
                "BootOrder": [
                    "Boot0019: Ubuntu",
                    "Boot0010: UEFI HTTPv4 (MAC:AA)"
                ]
            }
        }));
        assert_eq!(
            patched,
            json!({
                "Boot": {
                    "BootOrder": ["Boot0019", "Boot0010"]
                }
            })
        );
    }

    #[test]
    fn password_expiration_gets_only_missing_seconds() {
        let expiring = |date: &str| json!({ "PasswordExpiration": date });
        for (date, repaired) in [
            ("2026-10-26T16:49+00:00", "2026-10-26T16:49:00+00:00"),
            ("2026-10-26T16:49Z", "2026-10-26T16:49:00Z"),
            ("2026-10-26T16:49:30+00:00", "2026-10-26T16:49:30+00:00"),
            ("XXXXXXXXXXTXX:XX+bad", "XXXXXXXXXXTXX:XX+bad"),
            ("", ""),
        ] {
            assert_eq!(
                add_password_expiration_seconds(expiring(date)),
                expiring(repaired),
                "{date}"
            );
        }
        assert_eq!(add_password_expiration_seconds(json!({})), json!({}));
    }

    #[test]
    fn normalize_pem_chain_type_fixes_only_the_misspelling() {
        assert_eq!(
            normalize_pem_chain_type(json!({ "CertificateType": "PEMChain" })),
            json!({ "CertificateType": "PEMchain" })
        );
        assert_eq!(
            normalize_pem_chain_type(json!({ "CertificateType": "PEM" })),
            json!({ "CertificateType": "PEM" })
        );
        assert_eq!(normalize_pem_chain_type(json!({})), json!({}));
    }

    #[test]
    fn dpu_certificate_collection_stubs_become_references() {
        let patched = normalize_dpu_certificate_collection_stubs(json!({
            "@odata.type": "#AccountService.v1_15_0.AccountService",
            "MultiFactorAuth": { "ClientCertificate": { "Certificates": {
                "@odata.id": "/redfish/v1/AccountService/MultiFactorAuth/ClientCertificate/Certificates",
                "@odata.type": "#CertificateCollection.CertificateCollection",
                "Members": [],
                "Members@odata.count": 0
            } } },
            "Named": {
                "@odata.id": "/redfish/v1/Named",
                "@odata.type": "#CertificateCollection.CertificateCollection",
                "Name": "Complete"
            }
        }));
        assert_eq!(
            patched["MultiFactorAuth"]["ClientCertificate"]["Certificates"],
            json!({ "@odata.id": "/redfish/v1/AccountService/MultiFactorAuth/ClientCertificate/Certificates" })
        );
        assert_eq!(patched["Named"]["Name"], "Complete");
    }

    #[test]
    fn only_an_led_object_is_removed() {
        assert_eq!(
            remove_invalid_power_supply_location_indicator_active(json!({
                "LocationIndicatorActive": { "FaultLed": "OFF", "PowerLed": "Solid" }
            })),
            json!({})
        );
        for valid in [json!(true), json!(false), json!(null)] {
            let document = json!({ "LocationIndicatorActive": valid });
            assert_eq!(
                remove_invalid_power_supply_location_indicator_active(document.clone()),
                document
            );
        }
    }
}
