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

//! The repair table: the document quirks as rewrites, in one place, applied
//! by [`CompatBmc`](crate::CompatBmc) to every document it reads.
//!
//! A rule names the type it applies to as a
//! [`Match::Type`](crate::Match::Type) does — `Chassis` for
//! `#Chassis.v1_22_0.Chassis` in any version, and not for another type of
//! the schema, such as `#Chassis.v1_22_0.Links` — the quirk that enables it
//! on the classified platform, and the rewrite. The rules for one type
//! apply in table order, and the caller's [`UserRule`]s after them.
//!
//! A rule is scoped by document type, not by the route the document was
//! fetched through: a repair for `SoftwareInventory` reaches a firmware
//! inventory member and a software inventory member alike.

use std::mem;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::PoisonError;

use serde_json::Map;
use serde_json::Value;

use crate::fixes;
use crate::BmcQuirks;
use crate::UserRule;

/// One document repair.
struct Rule {
    /// The type the rule applies to, named as [`TypeName::is_named`] reads
    /// it.
    resource_type: &'static str,
    /// The quirk that enables it on the classified platform.
    enabled: fn(&BmcQuirks) -> bool,
    fix: fn(Value) -> Value,
}

const RULES: &[Rule] = &[
    Rule {
        resource_type: "UpdateService",
        enabled: BmcQuirks::bug_missing_update_service_name_field,
        fix: fixes::add_default_update_service_name,
    },
    Rule {
        resource_type: "SoftwareInventory",
        enabled: BmcQuirks::fw_inventory_wrong_release_date,
        fix: fixes::remove_wrong_release_date,
    },
    Rule {
        resource_type: "Chassis",
        enabled: BmcQuirks::bug_invalid_contained_by_fields,
        fix: fixes::remove_invalid_contained_by_fields,
    },
    Rule {
        resource_type: "Chassis",
        enabled: BmcQuirks::bug_missing_chassis_type_field,
        fix: fixes::add_default_chassis_type,
    },
    Rule {
        resource_type: "Chassis",
        enabled: BmcQuirks::bug_missing_chassis_name_field,
        fix: fixes::add_default_chassis_name,
    },
    Rule {
        resource_type: "Chassis",
        enabled: BmcQuirks::bug_empty_uuid_field,
        fix: fixes::normalize_empty_uuid_field,
    },
    Rule {
        resource_type: "ComputerSystem",
        enabled: BmcQuirks::computer_systems_wrong_last_reset_time,
        fix: fixes::remove_wrong_last_reset_time,
    },
    Rule {
        resource_type: "ComputerSystem",
        enabled: BmcQuirks::bug_empty_uuid_field,
        fix: fixes::normalize_empty_uuid_field,
    },
    Rule {
        resource_type: "ComputerSystem",
        enabled: BmcQuirks::vera_rubin_composite_boot_order_entries,
        fix: fixes::normalize_vera_rubin_composite_boot_order,
    },
    Rule {
        resource_type: "ManagerAccount",
        enabled: BmcQuirks::bug_no_account_type_in_accounts,
        fix: fixes::append_default_account_type,
    },
    // Keyed on the AccountService rather than on the collections it
    // inlines: a complete collection read on its own is left alone.
    Rule {
        resource_type: "AccountService",
        enabled: BmcQuirks::bug_incomplete_account_service_certificate_collection,
        fix: fixes::normalize_dpu_certificate_collection_stubs,
    },
    Rule {
        resource_type: "Certificate",
        enabled: BmcQuirks::certificate_type_wrong_pem_chain_case,
        fix: fixes::normalize_pem_chain_type,
    },
    // The Viking labels the certificate itself a `CertificateCollection`.
    Rule {
        resource_type: "CertificateCollection",
        enabled: BmcQuirks::certificate_type_wrong_pem_chain_case,
        fix: fixes::normalize_pem_chain_type,
    },
];

/// The rules a platform enables, one flag per rule in the table, and the
/// id of that set.
#[derive(Clone, Debug)]
pub struct Enabled {
    flags: Arc<[bool]>,
    id: u64,
}

impl Enabled {
    /// Whether no rule is enabled.
    pub const fn is_empty(&self) -> bool {
        self.id == 0
    }

    /// The id of the set: all a read's platform repairs depend on, so it
    /// identifies them too. Equal sets have one id in the process and
    /// different sets different ones; the empty set's is `0`.
    pub const fn id(&self) -> u64 {
        self.id
    }

    /// The enabled rules, in table order.
    fn rules(&self) -> impl Iterator<Item = &'static Rule> + '_ {
        RULES
            .iter()
            .zip(self.flags.iter())
            .filter(|(_, enabled)| **enabled)
            .map(|(rule, _)| rule)
    }
}

/// The rules `quirks` enables. A platform that needs no document repair
/// has none.
pub fn enabled(quirks: &BmcQuirks) -> Enabled {
    // Every set of rules a platform in the process enables, each once. A
    // set's id is its place here, so ids stay distinct however many rules
    // the table has.
    static SETS: Mutex<Vec<Arc<[bool]>>> = Mutex::new(Vec::new());

    let flags: Arc<[bool]> = RULES.iter().map(|rule| (rule.enabled)(quirks)).collect();
    if !flags.contains(&true) {
        return Enabled { flags, id: 0 };
    }
    let index = {
        let mut sets = SETS.lock().unwrap_or_else(PoisonError::into_inner);
        sets.iter()
            .position(|set| *set == flags)
            .unwrap_or_else(|| {
                sets.push(Arc::clone(&flags));
                sets.len() - 1
            })
    };
    Enabled {
        flags,
        id: index as u64 + 1,
    }
}

/// A type as an `@odata.type` names it, version aside: in
/// `#Chassis.v1_22_0.Links`, the type `Links` of the schema `Chassis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeName<'a> {
    schema: &'a str,
    name: &'a str,
}

impl<'a> TypeName<'a> {
    /// The type `odata_type` names, with or without its `#` and version:
    /// `#Chassis.v1_22_0.Chassis` and `Chassis.Chassis` name one type.
    pub fn parse(odata_type: &'a str) -> Option<Self> {
        let qualified = odata_type.strip_prefix('#').unwrap_or(odata_type);
        let (schema, name) = qualified.rsplit_once('.')?;
        let schema = schema
            .rsplit_once('.')
            .filter(|(_, version)| is_version(version))
            .map_or(schema, |(schema, _)| schema);
        (!schema.is_empty() && !name.is_empty()).then_some(Self { schema, name })
    }

    /// The type an object's `@odata.type` names.
    fn of(object: &'a Map<String, Value>) -> Option<Self> {
        Self::parse(object.get("@odata.type")?.as_str()?)
    }

    /// Whether this is the type `named`: a schema and a type in it,
    /// `Chassis.Links`, or a resource type by its schema alone, `Chassis`
    /// for `Chassis.Chassis`.
    pub fn is_named(self, named: &str) -> bool {
        TypeName::parse(named).map_or_else(
            || self.schema == named && self.name == named,
            |named| named == self,
        )
    }
}

/// Whether a segment of an `@odata.type` is a version, such as `v1_22_0`.
fn is_version(segment: &str) -> bool {
    segment
        .strip_prefix('v')
        .is_some_and(|number| number.starts_with(|c: char| c.is_ascii_digit()))
}

/// The document's `@odata.id`, unless the object is a bare reference: a
/// link names a document, it is not one.
fn document_id_of(object: &Map<String, Value>) -> Option<&str> {
    let id = object.get("@odata.id")?.as_str()?;
    (object.len() > 1).then_some(id)
}

/// Applies the platform's `enabled` repairs, then `user`'s, to every
/// object in `value` that is a document, the object before its children,
/// so a member a device expanded inline is repaired at any depth. An object
/// no rule applies to is left where it is, untouched.
pub fn repair_in_place(enabled: &Enabled, user: &[UserRule], value: &mut Value) {
    match value {
        Value::Object(object) => {
            let type_name = TypeName::of(object);
            // Collected while the object is borrowed; an empty list does not
            // allocate, so an object nothing applies to costs no allocation.
            let platform: Vec<fn(Value) -> Value> = type_name.map_or_else(Vec::new, |type_name| {
                enabled
                    .rules()
                    .filter(|rule| type_name.is_named(rule.resource_type))
                    .map(|rule| rule.fix)
                    .collect()
            });
            let applying: Vec<&UserRule> = if user.is_empty() {
                Vec::new()
            } else {
                let odata_id = document_id_of(object);
                user.iter()
                    .filter(|rule| rule.applies(type_name, odata_id))
                    .collect()
            };
            if !platform.is_empty() || !applying.is_empty() {
                let taken = mem::take(value);
                let taken = platform.into_iter().fold(taken, |value, fix| fix(value));
                *value = applying
                    .into_iter()
                    .fold(taken, |value, rule| rule.apply(value));
            }
            if let Value::Object(members) = value {
                for child in members.values_mut() {
                    repair_in_place(enabled, user, child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                repair_in_place(enabled, user, item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::IdPattern;
    use crate::Match;
    use crate::RootEvidence;

    fn enabled_for(root: &Value) -> Enabled {
        enabled(&BmcQuirks::classify(&RootEvidence::from_root(root)))
    }

    fn repaired(enabled: &Enabled, user: &[UserRule], mut value: Value) -> Value {
        repair_in_place(enabled, user, &mut value);
        value
    }

    #[test]
    fn a_type_is_its_schema_and_name_in_any_version() {
        let named = |odata_type: &str, named: &str| {
            TypeName::parse(odata_type).is_some_and(|type_name| type_name.is_named(named))
        };
        let inventory = "#SoftwareInventory.v1_4_0.SoftwareInventory";
        assert!(named(inventory, "SoftwareInventory"));
        assert!(named(inventory, "SoftwareInventory.SoftwareInventory"));
        assert!(named(
            inventory,
            "#SoftwareInventory.v1_0_0.SoftwareInventory"
        ));
        assert!(named(
            "#SoftwareInventoryCollection.SoftwareInventoryCollection",
            "SoftwareInventoryCollection"
        ));
        assert!(named("Chassis.v1_2_0.Chassis", "Chassis"));
        assert!(named("#Chassis.v1_22_0.Links", "Chassis.Links"));
        assert!(named(
            "#NvidiaChassis.v1_4_0.NvidiaCBCChassis",
            "NvidiaChassis.NvidiaCBCChassis"
        ));

        // Another type of the schema is not its resource type.
        assert!(!named("#Chassis.v1_22_0.Links", "Chassis"));
        assert!(!named("#Chassis.v1_22_0.Chassis", "Chassis.Links"));
        assert!(!named("#ChassisCollection.ChassisCollection", "Chassis"));

        assert!(TypeName::parse("#").is_none());
        assert!(TypeName::parse("#Chassis").is_none());
    }

    #[test]
    fn a_platform_without_document_repairs_enables_none() {
        assert!(enabled_for(&json!({ "Vendor": "Contoso" })).is_empty());
        assert!(enabled_for(&json!({ "Vendor": "Supermicro" })).is_empty());
        assert!(enabled_for(&json!({ "Vendor": "LITE-ON Technology" })).is_empty());
        assert!(!enabled_for(&json!({ "Vendor": "Dell" })).is_empty());
    }

    #[test]
    fn platforms_enabling_the_same_rules_share_an_id() {
        let dell = enabled_for(&json!({ "Vendor": "Dell" }));
        assert_eq!(
            dell.id(),
            enabled_for(&json!({ "Vendor": "Dell", "Product": "PowerEdge R760" })).id()
        );
        assert_ne!(dell.id(), enabled_for(&json!({ "Vendor": "HPE" })).id());
        assert_eq!(enabled_for(&json!({ "Vendor": "Contoso" })).id(), 0);
    }

    #[test]
    fn a_repair_reaches_a_member_expanded_inline() {
        let collection = json!({
            "@odata.id": "/redfish/v1/UpdateService/FirmwareInventory",
            "@odata.type": "#SoftwareInventoryCollection.SoftwareInventoryCollection",
            "Members": [{
                "@odata.id": "/redfish/v1/UpdateService/FirmwareInventory/BMC",
                "@odata.type": "#SoftwareInventory.v1_4_0.SoftwareInventory",
                "Id": "BMC",
                "ReleaseDate": "00:00:00Z",
            }],
        });

        let dell = enabled_for(&json!({ "Vendor": "Dell" }));
        let mended = repaired(&dell, &[], collection.clone());
        assert!(mended["Members"][0].get("ReleaseDate").is_none());
        assert_eq!(mended["Members"][0]["Id"], "BMC");

        // Another platform's documents are left exactly as they came.
        let hpe = enabled_for(&json!({ "Vendor": "HPE" }));
        assert_eq!(repaired(&hpe, &[], collection.clone()), collection);
    }

    #[test]
    fn the_repairs_for_one_type_apply_in_table_order() {
        let viking = enabled_for(&json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" }));
        let chassis = json!({ "@odata.type": "#Chassis.v1_2_0.Chassis", "Id": "1U" });

        let patched = repaired(&viking, &[], chassis.clone());
        assert_eq!(patched["ChassisType"], "Other");
        assert_eq!(patched["Name"], "Unnamed chassis");

        let hpe = enabled_for(&json!({ "Vendor": "HPE" }));
        assert_eq!(repaired(&hpe, &[], chassis.clone()), chassis);
    }

    #[test]
    fn a_rule_does_not_reach_another_type_of_its_schema() {
        let viking = enabled_for(&json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" }));
        let links = json!({ "@odata.type": "#Chassis.v1_2_0.Links", "Contains": [] });
        let chassis = json!({
            "@odata.type": "#Chassis.v1_2_0.Chassis",
            "Id": "1U",
            "Links": links,
        });

        let patched = repaired(&viking, &[], chassis);
        assert_eq!(patched["ChassisType"], "Other");
        assert_eq!(patched["Links"], links);
    }

    #[test]
    fn the_viking_certificate_is_repaired_under_either_label() {
        let viking = enabled_for(&json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" }));
        for odata_type in [
            "#Certificate.v1_8_0.Certificate",
            "#CertificateCollection.CertificateCollection",
        ] {
            let certificate = json!({ "@odata.type": odata_type, "CertificateType": "PEMChain" });
            assert_eq!(
                repaired(&viking, &[], certificate)["CertificateType"],
                "PEMchain"
            );
        }
    }

    #[test]
    fn user_rules_follow_the_platforms_and_skip_references() {
        let viking = enabled_for(&json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" }));
        let user = [
            // Runs after the platform rule that names the chassis.
            UserRule::new("rename", Match::Type("Chassis".into()), |mut v| {
                v["Name"] = json!(format!("{} (renamed)", v["Name"].as_str().unwrap_or("")));
                v
            }),
            UserRule::new(
                "tag",
                Match::Id(IdPattern::new("/redfish/v1/Chassis/*")),
                |mut v| {
                    v["Tagged"] = json!(true);
                    v
                },
            ),
        ];
        let document = json!({
            "@odata.id": "/redfish/v1/Chassis/1U",
            "@odata.type": "#Chassis.v1_2_0.Chassis",
            "Id": "1U",
            "Links": { "Contains": [{ "@odata.id": "/redfish/v1/Chassis/2U" }] },
        });

        let patched = repaired(&viking, &user, document);
        assert_eq!(patched["Name"], "Unnamed chassis (renamed)");
        assert_eq!(patched["Tagged"], true);
        assert_eq!(
            patched["Links"]["Contains"][0],
            json!({ "@odata.id": "/redfish/v1/Chassis/2U" })
        );
    }
}
