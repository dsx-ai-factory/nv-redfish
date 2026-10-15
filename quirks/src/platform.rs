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

//! The quirks an endpoint has, decided from what its service root says:
//! every rule the root matches adds its quirks.

use serde_json::Value;

use crate::fixes;

/// What a service root says about the device: the evidence the quirk
/// rules match, borrowed from the raw root with
/// [`RootEvidence::from_root`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RootEvidence<'a> {
    /// The root's `Vendor`.
    pub vendor: Option<&'a str>,
    /// The root's `Product`.
    pub product: Option<&'a str>,
    /// The root's `RedfishVersion`.
    pub redfish_version: Option<&'a str>,
    /// The root's `Oem.Ami.RtpVersion`, which the GB300 host BMC exposes
    /// and other AMI BMCs do not.
    pub ami_rtp_version: Option<&'a str>,
    /// Whether the root carries an `Oem.Ami` object, as Lenovo trays
    /// running AMI firmware do under their own vendor.
    pub ami_oem: bool,
}

impl<'a> RootEvidence<'a> {
    /// The evidence a raw service root document carries.
    #[must_use]
    pub fn from_root(root: &'a Value) -> Self {
        let text = |pointer: &str| root.pointer(pointer).and_then(Value::as_str);
        Self {
            vendor: text("/Vendor"),
            product: text("/Product"),
            redfish_version: text("/RedfishVersion"),
            ami_rtp_version: text("/Oem/Ami/RtpVersion"),
            ami_oem: root.pointer("/Oem/Ami").is_some(),
        }
    }

    fn vendor_is(&self, vendor: &str) -> bool {
        self.vendor == Some(vendor)
    }

    fn product_is(&self, product: &str) -> bool {
        self.product == Some(product)
    }

    fn redfish_version_is(&self, version: &str) -> bool {
        self.redfish_version == Some(version)
    }

    fn ami_rtp_version_is(&self, version: &str) -> bool {
        self.ami_rtp_version == Some(version)
    }
}

// One quirk a device can have; each is read by one `BmcQuirks` predicate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Quirk {
    FilteredLogEntriesCount,
    NoAccountTypeInAccounts,
    IncompleteAccountServiceCertificateCollection,
    FwInventoryWrongReleaseDate,
    JobLocationInCreateResponse,
    InvalidContainedByFields,
    MissingRootNavProperties,
    MissingChassisTypeField,
    MissingChassisNameField,
    EmptyUuidField,
    DpuOemComputerSystem,
    MissingUpdateServiceNameField,
    CertificateTypeWrongPemChainCase,
    ComputerSystemsWrongLastResetTime,
    SseNoMemberId,
    SseEmptyLogEntry,
    SseWrongTimestampOffset,
    SseMissingEventType,
    SseMissingEnvelopeIds,
    CompositeBootOrderEntries,
    WrongComputerSystemMembers,
    WrongManagerMembers,
    ResetToDefaultsTypeParameter,
    PowerSupplyLocationIndicatorObject,
    ExpandIsNotWorkingProperly,
    NullableMembers,
    RejectsOwnEtag,
    AcceptedUpdateWithoutTask,
}

// Devices whose service root `covers` matches have `quirks`.
struct Rule {
    covers: fn(&RootEvidence<'_>) -> bool,
    quirks: &'static [Quirk],
}

// A device has the quirks of every rule its service root matches. A rule
// only ever adds quirks, so a rule for one product cannot take away the
// quirks the rules for its vendor give it. A product without one of its
// vendor's quirks is excluded by name in that quirk's rule.
const RULES: &[Rule] = &[
    Rule {
        covers: |root| root.vendor_is("HPE"),
        quirks: &[Quirk::NoAccountTypeInAccounts],
    },
    Rule {
        covers: |root| root.vendor_is("Dell"),
        quirks: &[
            Quirk::FwInventoryWrongReleaseDate,
            Quirk::JobLocationInCreateResponse,
            Quirk::ComputerSystemsWrongLastResetTime,
            Quirk::SseWrongTimestampOffset,
        ],
    },
    // AMI MegaRAC firmware, including Lenovo trays (HS350x class) that keep
    // the Lenovo vendor but expose an AMI OEM object in the service root.
    Rule {
        covers: |root| root.vendor_is("AMI") || (root.vendor_is("Lenovo") && root.ami_oem),
        quirks: &[Quirk::RejectsOwnEtag],
    },
    // The H100 Viking.
    Rule {
        covers: |root| root.vendor_is("AMI") && root.redfish_version_is("1.11.0"),
        quirks: &[
            Quirk::InvalidContainedByFields,
            Quirk::MissingRootNavProperties,
            Quirk::MissingChassisTypeField,
            Quirk::MissingChassisNameField,
            Quirk::MissingUpdateServiceNameField,
            Quirk::CertificateTypeWrongPemChainCase,
            Quirk::WrongComputerSystemMembers,
            Quirk::WrongManagerMembers,
            Quirk::ExpandIsNotWorkingProperly,
        ],
    },
    // The GB300 host BMC on AMI firmware, told apart from other AMI BMCs by
    // the OEM `RtpVersion` its service root exposes.
    Rule {
        covers: |root| root.vendor_is("AMI") && root.ami_rtp_version_is("13.09.1"),
        quirks: &[Quirk::ExpandIsNotWorkingProperly],
    },
    // NVIDIA's BMC firmware.
    Rule {
        covers: |root| root.vendor_is("NVIDIA"),
        quirks: &[Quirk::SseNoMemberId, Quirk::SseMissingEventType],
    },
    // NVIDIA's BMC firmware, except on the NVLink switch tray.
    Rule {
        covers: |root| root.vendor_is("NVIDIA") && !root.product_is("P3809"),
        quirks: &[Quirk::FilteredLogEntriesCount],
    },
    // The Vera Rubin host BMC.
    Rule {
        covers: |root| root.vendor_is("NVIDIA") && root.product_is("VR NVL72"),
        quirks: &[
            Quirk::SseEmptyLogEntry,
            Quirk::SseMissingEnvelopeIds,
            Quirk::CompositeBootOrderEntries,
        ],
    },
    // The GB300 host BMC on NVIDIA's firmware.
    Rule {
        covers: |root| root.vendor_is("NVIDIA") && root.product_is("GB BMC"),
        quirks: &[Quirk::ExpandIsNotWorkingProperly],
    },
    // BlueField DPUs: their service roots use an `Nvidia` vendor with these
    // product names; BlueField-4 reports `BlueField-4`.
    Rule {
        covers: |root| {
            root.vendor_is("Nvidia")
                && (root.product_is("Nvidia-BMCMezz")
                    || root.product_is("BlueField-3 DPU")
                    || root.product_is("BlueField-4"))
        },
        quirks: &[
            Quirk::IncompleteAccountServiceCertificateCollection,
            Quirk::EmptyUuidField,
            Quirk::DpuOemComputerSystem,
            Quirk::NullableMembers,
        ],
    },
    // Wiwynn ODM GB200 NVL trays, which report their own vendor rather
    // than `NVIDIA`.
    Rule {
        covers: |root| root.vendor_is("WIWYNN"),
        quirks: &[Quirk::SseNoMemberId, Quirk::SseMissingEventType],
    },
    Rule {
        covers: |root| root.vendor_is("Supermicro"),
        quirks: &[Quirk::AcceptedUpdateWithoutTask],
    },
    // Lite-On power shelves that name their vendor.
    Rule {
        covers: |root| {
            root.vendor
                .is_some_and(|vendor| vendor.starts_with("LITE-ON"))
        },
        quirks: &[
            Quirk::MissingRootNavProperties,
            Quirk::ResetToDefaultsTypeParameter,
            Quirk::PowerSupplyLocationIndicatorObject,
        ],
    },
    // A service root that names no vendor and reports Redfish 1.9.0, as
    // Lite-On power shelves often do.
    Rule {
        covers: |root| root.vendor.is_none() && root.redfish_version_is("1.9.0"),
        quirks: &[Quirk::MissingRootNavProperties],
    },
];

/// The quirks of one endpoint. Decided once, from its service root, and
/// consulted for every workaround afterwards.
#[derive(Debug)]
pub struct BmcQuirks {
    quirks: Vec<Quirk>,
}

impl BmcQuirks {
    /// The quirks of the endpoint whose service root carries `evidence`:
    /// those of every rule it matches. An endpoint no rule matches has no
    /// quirks.
    #[must_use]
    pub fn classify(evidence: &RootEvidence<'_>) -> Self {
        let mut quirks = Vec::new();
        for rule in RULES.iter().filter(|rule| (rule.covers)(evidence)) {
            for quirk in rule.quirks {
                if !quirks.contains(quirk) {
                    quirks.push(*quirk);
                }
            }
        }
        Self { quirks }
    }

    fn has(&self, quirk: Quirk) -> bool {
        self.quirks.contains(&quirk)
    }

    /// A filtered log entry collection can report the unfiltered count in
    /// `Members@odata.count`.
    #[must_use]
    pub fn bug_filtered_log_entries_count(&self) -> bool {
        self.has(Quirk::FilteredLogEntriesCount)
    }

    /// `AccountTypes` is required by the schema, but some vendors omit it,
    /// on account reads and on the account a create or update answers
    /// with.
    #[must_use]
    pub fn bug_no_account_type_in_accounts(&self) -> bool {
        self.has(Quirk::NoAccountTypeInAccounts)
    }

    /// A DPU inlines the AccountService's certificate collections as
    /// stubs that lack the `Name` every resource requires.
    pub(crate) fn bug_incomplete_account_service_certificate_collection(&self) -> bool {
        self.has(Quirk::IncompleteAccountServiceCertificateCollection)
    }

    /// `ReleaseDate` in the firmware inventory is `00:00:00Z`, which is not
    /// a `DateTimeOffset`.
    pub(crate) fn fw_inventory_wrong_release_date(&self) -> bool {
        self.has(Quirk::FwInventoryWrongReleaseDate)
    }

    /// iDRAC answers a Volume create with 200 or 201 and the URI of the
    /// Dell job that will build the volume (in `Location` or as the body's
    /// `@odata.id`) instead of 202 Accepted.
    #[must_use]
    pub fn bug_job_location_in_create_response(&self) -> bool {
        self.has(Quirk::JobLocationInCreateResponse)
    }

    /// `Links.ContainedBy` on a chassis carries fields beyond `@odata.id`.
    pub(crate) fn bug_invalid_contained_by_fields(&self) -> bool {
        self.has(Quirk::InvalidContainedByFields)
    }

    /// The service root omits navigation properties that a direct GET
    /// still serves: Vikings before a BMC reset, Lite-On power shelves for
    /// `/Systems`.
    #[must_use]
    pub fn bug_missing_root_nav_properties(&self) -> bool {
        self.has(Quirk::MissingRootNavProperties)
    }

    /// A chassis omits `ChassisType`, which the schema requires.
    pub(crate) fn bug_missing_chassis_type_field(&self) -> bool {
        self.has(Quirk::MissingChassisTypeField)
    }

    /// A chassis omits `Name`, which every resource requires.
    pub(crate) fn bug_missing_chassis_name_field(&self) -> bool {
        self.has(Quirk::MissingChassisNameField)
    }

    /// A DPU in NIC mode reports an empty-string `UUID` on chassis and
    /// computer system documents.
    pub(crate) fn bug_empty_uuid_field(&self) -> bool {
        self.has(Quirk::EmptyUuidField)
    }

    /// A DPU serves the computer system's `Oem.Nvidia` object as a separate
    /// resource, inlining only a partially expanded stub. The extra fetch
    /// is restricted to this platform.
    #[must_use]
    pub fn bug_dpu_oem_computer_system(&self) -> bool {
        self.has(Quirk::DpuOemComputerSystem)
    }

    /// The update service omits `Name`, which every resource requires.
    pub(crate) fn bug_missing_update_service_name_field(&self) -> bool {
        self.has(Quirk::MissingUpdateServiceNameField)
    }

    /// The H100 Viking reports the standard `PEMchain` certificate type as
    /// `PEMChain`.
    pub(crate) fn certificate_type_wrong_pem_chain_case(&self) -> bool {
        self.has(Quirk::CertificateTypeWrongPemChainCase)
    }

    /// `LastResetTime` on a computer system is `0000-00-00T00:00:00+00:00`,
    /// which is not a `DateTimeOffset`.
    pub(crate) fn computer_systems_wrong_last_reset_time(&self) -> bool {
        self.has(Quirk::ComputerSystemsWrongLastResetTime)
    }

    /// Event records in the SSE payload omit `MemberId`.
    pub(crate) fn event_service_sse_no_member_id(&self) -> bool {
        self.has(Quirk::SseNoMemberId)
    }

    /// Event records in the SSE payload carry an empty object instead of a
    /// `LogEntry` reference.
    pub(crate) fn event_service_sse_empty_log_entry(&self) -> bool {
        self.has(Quirk::SseEmptyLogEntry)
    }

    /// Event records in the SSE payload stamp `EventTimestamp` with a
    /// compact timezone offset such as `-0600`.
    pub(crate) fn event_service_sse_wrong_timestamp_offset(&self) -> bool {
        self.has(Quirk::SseWrongTimestampOffset)
    }

    /// Event records in the SSE payload omit `EventType`.
    pub(crate) fn event_service_sse_missing_event_type(&self) -> bool {
        self.has(Quirk::SseMissingEventType)
    }

    /// Event envelopes omit both `Id` and `@odata.id` while the stream
    /// carries an SSE id.
    pub(crate) fn event_service_sse_missing_envelope_ids(&self) -> bool {
        self.has(Quirk::SseMissingEnvelopeIds)
    }

    /// Vera Rubin host BMCs report composite `BootOrder` entries such as
    /// `"Boot0019: Ubuntu"` while boot option resources use the bare
    /// reference.
    pub(crate) fn vera_rubin_composite_boot_order_entries(&self) -> bool {
        self.has(Quirk::CompositeBootOrderEntries)
    }

    /// Vikings list wrong members in the computer system collection; the
    /// filter keeps the ones that are systems.
    #[must_use]
    pub fn filter_computer_system_odata_ids(&self) -> Option<fn(&str) -> bool> {
        self.has(Quirk::WrongComputerSystemMembers)
            .then_some(|odata_id| {
                odata_id.ends_with("/DGX") || odata_id.ends_with("/HGX_Baseboard_0")
            })
    }

    /// Vikings list wrong members in the manager collection; the filter
    /// keeps the ones that are managers.
    #[must_use]
    pub fn filter_manager_odata_ids(&self) -> Option<fn(&str) -> bool> {
        self.has(Quirk::WrongManagerMembers).then_some(|odata_id| {
            odata_id.ends_with("/BMC")
                || odata_id.ends_with("/HGX_BMC_0")
                || odata_id.ends_with("/HGX_FabricManager_0")
        })
    }

    /// Lite-On power shelves advertise `ResetType` for
    /// `Manager.ResetToDefaults` but reject it, requiring the parameter to
    /// be named `ResetToDefaultsType`.
    ///
    /// Their service root often omits `Vendor` and is then
    /// indistinguishable from a Delta shelf, so the manager's own
    /// `Manufacturer` is checked too.
    #[must_use]
    pub fn bug_reset_to_defaults_type_parameter(&self, manager_manufacturer: Option<&str>) -> bool {
        self.has(Quirk::ResetToDefaultsTypeParameter) || is_liteon(manager_manufacturer)
    }

    /// Lite-On PMC 2.3.3+ reports `PowerSupply.LocationIndicatorActive` as
    /// an LED-state object instead of a boolean. Returns the repair when the
    /// service root, or the manufacturer of the chassis holding the
    /// supplies, says Lite-On.
    ///
    /// This is the one document repair decided per resource rather than
    /// from the service root: a shelf's root often omits `Vendor`, and is
    /// then told apart from a Delta shelf only by its chassis. The chassis
    /// wrapper applies it to the supplies it reads.
    #[must_use]
    pub fn power_supply_location_indicator_repair(
        &self,
        chassis_manufacturer: Option<&str>,
    ) -> Option<fn(Value) -> Value> {
        (self.has(Quirk::PowerSupplyLocationIndicatorObject) || is_liteon(chassis_manufacturer))
            .then_some(
                fixes::remove_invalid_power_supply_location_indicator_active as fn(Value) -> Value,
            )
    }

    /// `$expand` responses drop required fields (`Id`, `Name`,
    /// `ChassisType`) from embedded members while the standalone GETs are
    /// complete, so expansion is not used and each member is fetched on its
    /// own. Seen on the Viking, and on GB300 host BMCs with AMI or NVIDIA
    /// firmware.
    #[must_use]
    pub fn expand_is_not_working_properly(&self) -> bool {
        self.has(Quirk::ExpandIsNotWorkingProperly)
    }

    /// A collection's `Members` is `null` rather than `[]`.
    #[must_use]
    pub fn bug_nullable_members(&self) -> bool {
        self.has(Quirk::NullableMembers)
    }

    /// AMI MegaRAC firmware requires `If-Match` on PATCH but rejects the
    /// ETag it served, accepting only `*`.
    pub(crate) fn bug_rejects_own_etag(&self) -> bool {
        self.has(Quirk::RejectsOwnEtag)
    }

    /// Supermicro answers a staged settings change with `202 Accepted` and
    /// nothing to poll: no `Location` and no Task.
    pub(crate) fn bug_accepted_update_without_task(&self) -> bool {
        self.has(Quirk::AcceptedUpdateWithoutTask)
    }
}

/// Whether a resource's `Manufacturer` names a Lite-On device.
fn is_liteon(manufacturer: Option<&str>) -> bool {
    manufacturer
        .is_some_and(|manufacturer| manufacturer.eq_ignore_ascii_case("LITE-ON TECHNOLOGY CORP."))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn quirks_of(root: &Value) -> BmcQuirks {
        BmcQuirks::classify(&RootEvidence::from_root(root))
    }

    #[test]
    #[allow(clippy::too_many_lines)] // One row per known service root.
    fn a_root_has_the_quirks_of_every_rule_it_matches() {
        let nvidia = [Quirk::SseNoMemberId, Quirk::SseMissingEventType];
        let dpu = [
            Quirk::IncompleteAccountServiceCertificateCollection,
            Quirk::EmptyUuidField,
            Quirk::DpuOemComputerSystem,
            Quirk::NullableMembers,
        ];
        for (root, expected) in [
            (
                json!({ "Vendor": "Dell" }),
                &[
                    Quirk::FwInventoryWrongReleaseDate,
                    Quirk::JobLocationInCreateResponse,
                    Quirk::ComputerSystemsWrongLastResetTime,
                    Quirk::SseWrongTimestampOffset,
                ][..],
            ),
            (
                json!({ "Vendor": "HPE" }),
                &[Quirk::NoAccountTypeInAccounts],
            ),
            (
                json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" }),
                &[
                    Quirk::RejectsOwnEtag,
                    Quirk::InvalidContainedByFields,
                    Quirk::MissingRootNavProperties,
                    Quirk::MissingChassisTypeField,
                    Quirk::MissingChassisNameField,
                    Quirk::MissingUpdateServiceNameField,
                    Quirk::CertificateTypeWrongPemChainCase,
                    Quirk::WrongComputerSystemMembers,
                    Quirk::WrongManagerMembers,
                    Quirk::ExpandIsNotWorkingProperly,
                ],
            ),
            (
                json!({ "Vendor": "AMI", "Oem": { "Ami": { "RtpVersion": "13.09.1" } } }),
                &[Quirk::RejectsOwnEtag, Quirk::ExpandIsNotWorkingProperly],
            ),
            (json!({ "Vendor": "AMI" }), &[Quirk::RejectsOwnEtag]),
            (
                json!({ "Vendor": "Lenovo", "Oem": { "Ami": {} } }),
                &[Quirk::RejectsOwnEtag],
            ),
            (json!({ "Vendor": "Lenovo" }), &[]),
            (
                json!({ "Vendor": "NVIDIA", "Product": "VR NVL72" }),
                &[
                    Quirk::SseNoMemberId,
                    Quirk::SseMissingEventType,
                    Quirk::FilteredLogEntriesCount,
                    Quirk::SseEmptyLogEntry,
                    Quirk::SseMissingEnvelopeIds,
                    Quirk::CompositeBootOrderEntries,
                ],
            ),
            (json!({ "Vendor": "NVIDIA", "Product": "P3809" }), &nvidia),
            (
                json!({ "Vendor": "NVIDIA", "Product": "GB BMC" }),
                &[
                    Quirk::SseNoMemberId,
                    Quirk::SseMissingEventType,
                    Quirk::FilteredLogEntriesCount,
                    Quirk::ExpandIsNotWorkingProperly,
                ],
            ),
            (
                json!({ "Vendor": "NVIDIA", "Product": "DGX" }),
                &[
                    Quirk::SseNoMemberId,
                    Quirk::SseMissingEventType,
                    Quirk::FilteredLogEntriesCount,
                ],
            ),
            (
                json!({ "Vendor": "Nvidia", "Product": "BlueField-3 DPU" }),
                &dpu,
            ),
            (
                json!({ "Vendor": "Nvidia", "Product": "BlueField-4" }),
                &dpu,
            ),
            (json!({ "Vendor": "Nvidia", "Product": "Other" }), &[]),
            (json!({ "Vendor": "WIWYNN" }), &nvidia),
            (
                json!({ "Vendor": "Supermicro" }),
                &[Quirk::AcceptedUpdateWithoutTask],
            ),
            (
                json!({ "Vendor": "LITE-ON Technology" }),
                &[
                    Quirk::MissingRootNavProperties,
                    Quirk::ResetToDefaultsTypeParameter,
                    Quirk::PowerSupplyLocationIndicatorObject,
                ],
            ),
            (
                json!({ "RedfishVersion": "1.9.0" }),
                &[Quirk::MissingRootNavProperties],
            ),
            (json!({ "Vendor": "Contoso" }), &[]),
            (json!({}), &[]),
        ] {
            let quirks = quirks_of(&root);
            assert_eq!(quirks.quirks.len(), expected.len(), "{root}: {quirks:?}");
            for quirk in expected {
                assert!(quirks.has(*quirk), "{} lacks {:?}", root, quirk);
            }
        }
    }

    #[test]
    fn a_product_keeps_the_quirks_of_its_vendor() {
        // A rule only adds, so whatever the product, an NVIDIA root keeps
        // the quirks of NVIDIA's firmware; the NVLink switch tray lacks the
        // filtered count because that quirk's rule names it.
        for product in [
            None,
            Some("VR NVL72"),
            Some("P3809"),
            Some("GB BMC"),
            Some("DGX"),
            Some("next year's"),
        ] {
            let root = json!({ "Vendor": "NVIDIA", "Product": product });
            let quirks = quirks_of(&root);
            assert!(quirks.event_service_sse_no_member_id(), "{}", root);
            assert!(quirks.event_service_sse_missing_event_type(), "{}", root);
            assert_eq!(
                quirks.bug_filtered_log_entries_count(),
                product != Some("P3809"),
                "{root}"
            );
        }
    }

    #[test]
    fn the_liteon_led_repair_follows_the_root_or_the_chassis() {
        let anonymous = quirks_of(&json!({ "RedfishVersion": "1.9.0" }));
        assert!(anonymous
            .power_supply_location_indicator_repair(Some("LITE-ON TECHNOLOGY CORP."))
            .is_some());
        assert!(anonymous
            .power_supply_location_indicator_repair(Some("ACME Corp."))
            .is_none());
        assert!(anonymous
            .power_supply_location_indicator_repair(None)
            .is_none());
        let liteon = quirks_of(&json!({ "Vendor": "LITE-ON Technology" }));
        assert!(liteon
            .power_supply_location_indicator_repair(None)
            .is_some());
    }
}
