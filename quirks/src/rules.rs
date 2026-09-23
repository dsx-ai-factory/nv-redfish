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

//! The repair table: the document quirks as rewrites, in one place, so the
//! wrappers and [`CompatBmc`](crate::CompatBmc) apply the same repairs to
//! the same resource type.
//!
//! A rule names the resource type it applies to — the family of the
//! document's `@odata.type`, `SoftwareInventory` for
//! `#SoftwareInventory.v1_4_0.SoftwareInventory` — the quirk that enables
//! it on the classified platform, and the rewrite. The rules for one type
//! apply in table order. A wrapper asks for its type by name; the
//! compatibility layer reads the type off each document it fetches.
//!
//! A rule is scoped by document type, not by the route the document was
//! fetched through: a repair for `SoftwareInventory` reaches a firmware
//! inventory member and a software inventory member alike.

use std::mem;
use std::sync::Arc;

use serde_json::Value;

use crate::fixes;
use crate::BmcQuirks;
use crate::ReadPatchFn;

/// One document repair.
struct Rule {
    /// The `@odata.type` family the rule applies to.
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
];

fn enabled_for<'a>(
    quirks: &'a BmcQuirks,
    resource_type: &'a str,
) -> impl Iterator<Item = &'a Rule> + 'a {
    RULES
        .iter()
        .filter(move |rule| rule.resource_type == resource_type && (rule.enabled)(quirks))
}

/// The repairs `quirks` enables on a `resource_type` document, composed in
/// table order; `None` when it enables none, so a caller can keep its
/// unpatched read path.
pub fn compose(quirks: &BmcQuirks, resource_type: &str) -> Option<ReadPatchFn> {
    let fixes: Vec<fn(Value) -> Value> = enabled_for(quirks, resource_type)
        .map(|rule| rule.fix)
        .collect();
    (!fixes.is_empty()).then(|| {
        Arc::new(move |value| fixes.iter().fold(value, |value, fix| fix(value))) as ReadPatchFn
    })
}

/// The resource type family a document's `@odata.type` names:
/// `#SoftwareInventory.v1_4_0.SoftwareInventory` is `SoftwareInventory`.
fn resource_type_of(value: &Value) -> Option<&str> {
    let odata_type = value.get("@odata.type")?.as_str()?;
    let family = odata_type
        .strip_prefix('#')
        .unwrap_or(odata_type)
        .split('.')
        .next()?;
    (!family.is_empty()).then_some(family)
}

/// Whether `quirks` enables any document repair at all: a platform that
/// needs none can be read without copying a document to walk it.
pub fn any_enabled(quirks: &BmcQuirks) -> bool {
    RULES.iter().any(|rule| (rule.enabled)(quirks))
}

/// Applies the enabled repairs to every object in `value` that carries an
/// `@odata.type`, the object before its children, so a member a device
/// expanded inline is repaired at any depth. An object no rule applies to
/// is left where it is, untouched.
pub fn repair_in_place(quirks: &BmcQuirks, value: &mut Value) {
    match value {
        Value::Object(_) => {
            if let Some(resource_type) = resource_type_of(value).map(str::to_owned) {
                let mut rules = enabled_for(quirks, &resource_type).peekable();
                if rules.peek().is_some() {
                    let taken = mem::take(value);
                    *value = rules.fold(taken, |value, rule| (rule.fix)(value));
                }
            }
            if let Value::Object(members) = value {
                for child in members.values_mut() {
                    repair_in_place(quirks, child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                repair_in_place(quirks, item);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::RootEvidence;

    fn quirks_of(root: &Value) -> BmcQuirks {
        BmcQuirks::classify(&RootEvidence::from_root(root))
    }

    fn repaired(quirks: &BmcQuirks, mut value: Value) -> Value {
        repair_in_place(quirks, &mut value);
        value
    }

    #[test]
    fn the_resource_type_is_the_odata_type_family() {
        let typed = |odata_type: &str| {
            resource_type_of(&json!({ "@odata.type": odata_type })).map(str::to_owned)
        };
        assert_eq!(
            typed("#SoftwareInventory.v1_4_0.SoftwareInventory"),
            Some("SoftwareInventory".to_owned())
        );
        assert_eq!(
            typed("#SoftwareInventoryCollection.SoftwareInventoryCollection"),
            Some("SoftwareInventoryCollection".to_owned())
        );
        assert_eq!(typed("Chassis.v1_2_0.Chassis"), Some("Chassis".to_owned()));
        assert_eq!(typed("#"), None);
        assert_eq!(
            resource_type_of(&json!({ "@odata.id": "/redfish/v1" })),
            None
        );
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

        let dell = quirks_of(&json!({ "Vendor": "Dell" }));
        assert!(any_enabled(&dell));
        let mended = repaired(&dell, collection.clone());
        assert!(mended["Members"][0].get("ReleaseDate").is_none());
        assert_eq!(mended["Members"][0]["Id"], "BMC");

        // Another platform's documents are left exactly as they came, and
        // a platform no rule names needs no walk at all.
        let contoso = quirks_of(&json!({ "Vendor": "Contoso" }));
        assert!(!any_enabled(&contoso));
        assert_eq!(repaired(&contoso, collection.clone()), collection);
        assert_eq!(
            repaired(&quirks_of(&json!({ "Vendor": "HPE" })), collection.clone()),
            collection
        );
    }

    #[test]
    fn the_repairs_for_one_type_compose_in_table_order() {
        let viking = quirks_of(&json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" }));

        let patch = compose(&viking, "Chassis").expect("Viking chassis need repairs");
        let patched = patch(json!({ "Id": "1U" }));
        assert_eq!(patched["ChassisType"], "Other");
        assert_eq!(patched["Name"], "Unnamed chassis");

        assert!(compose(&quirks_of(&json!({ "Vendor": "HPE" })), "Chassis").is_none());
        assert!(compose(&viking, "ChassisCollection").is_none());
    }
}
