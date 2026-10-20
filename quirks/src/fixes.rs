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

use nv_redfish_core::EdmGuid;
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

/// `Status.State` `Standby`, which is not a `Resource.State`, is read as
/// `StandbySpare`; every other state stays.
pub fn normalize_standby_state(mut v: Value) -> Value {
    if let Some(state) = v.pointer_mut("/Status/State") {
        if state == "Standby" {
            *state = Value::String("StandbySpare".into());
        }
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
