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

//! Platform classification: which class of BMC an endpoint is, decided
//! from its service root, and the quirks that class is known for.

use serde_json::Value;

use crate::rules;
use crate::ReadPatchFn;

/// What a service root says about the device: the evidence classification
/// runs on.
///
/// Built from the raw root with [`RootEvidence::from_root`], or by a caller
/// that already holds a typed root — `nv-redfish`'s `ServiceRoot` does, so
/// a new field lands in both places.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootEvidence {
    /// The root's `Vendor`.
    pub vendor: Option<String>,
    /// The root's `Product`.
    pub product: Option<String>,
    /// The root's `RedfishVersion`.
    pub redfish_version: Option<String>,
    /// The root's `Oem.Ami.RtpVersion`, which the GB300 host BMC exposes
    /// and other AMI BMCs do not.
    pub ami_rtp_version: Option<String>,
}

impl RootEvidence {
    /// The evidence a raw service root document carries.
    #[must_use]
    pub fn from_root(root: &Value) -> Self {
        let text = |pointer: &str| {
            root.pointer(pointer)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        Self {
            vendor: text("/Vendor"),
            product: text("/Product"),
            redfish_version: text("/RedfishVersion"),
            ami_rtp_version: text("/Oem/Ami/RtpVersion"),
        }
    }
}

// A platform is not a vendor: it is a class of devices that share one set
// of quirks.
#[derive(Debug, PartialEq, Eq)]
enum Platform {
    Hpe,
    Dell,
    AmiViking,
    AmiGb300,
    VeraRubin,
    Nvidia,
    NvidiaDpu,
    Wiwynn,
    Anonymous1_9_0,
    LiteonPowershelf,
    NvSwitch,
}

/// The quirks of one classified platform. Classified once, from the
/// service root, and consulted for every workaround afterwards.
#[derive(Debug)]
pub struct BmcQuirks {
    platform: Option<Platform>,
}

impl BmcQuirks {
    /// Classifies the platform from its service root's evidence. An
    /// endpoint no class matches has no quirks.
    #[must_use]
    pub fn classify(evidence: &RootEvidence) -> Self {
        let vendor = evidence.vendor.as_deref();
        let product = evidence.product.as_deref();
        let redfish_version = evidence.redfish_version.as_deref();
        let rtp_version = evidence.ami_rtp_version.as_deref();
        let platform = match vendor {
            Some("HPE") => Some(Platform::Hpe),
            Some("Dell") => Some(Platform::Dell),
            Some("AMI") if redfish_version == Some("1.11.0") => Some(Platform::AmiViking),
            // The GB300 host BMC exposes an AMI OEM `RtpVersion` in the
            // service root, which tells it apart from other AMI BMCs so the
            // expand workaround is not applied to every AMI platform.
            Some("AMI") if rtp_version == Some("13.09.1") => Some(Platform::AmiGb300),
            Some("NVIDIA") if product == Some("VR NVL72") => Some(Platform::VeraRubin),
            Some("NVIDIA") if product == Some("P3809") => Some(Platform::NvSwitch),
            Some("NVIDIA") => Some(Platform::Nvidia),
            // BF3 service roots use this product name with an `Nvidia` vendor.
            Some("Nvidia") if matches!(product, Some("Nvidia-BMCMezz" | "BlueField-3 DPU")) => {
                Some(Platform::NvidiaDpu)
            }
            // Wiwynn ODM GB200 NVL trays report their own vendor rather than
            // `NVIDIA`.
            Some("WIWYNN") => Some(Platform::Wiwynn),
            Some(vendor) if vendor.starts_with("LITE-ON") => Some(Platform::LiteonPowershelf),
            None if redfish_version == Some("1.9.0") => Some(Platform::Anonymous1_9_0),
            _ => None,
        };
        Self { platform }
    }

    /// The document repairs this platform needs on one `resource_type`,
    /// composed in table order; `None` when it needs none.
    #[must_use]
    pub fn read_patch(&self, resource_type: &str) -> Option<ReadPatchFn> {
        rules::compose(self, resource_type)
    }

    /// `AccountTypes` is required by the schema, but some vendors omit it;
    /// the account service substitutes the schema's default.
    #[must_use]
    pub fn bug_no_account_type_in_accounts(&self) -> bool {
        self.platform == Some(Platform::Hpe)
    }

    /// `ReleaseDate` in the firmware inventory is `00:00:00Z`, which is not
    /// a `DateTimeOffset`.
    #[must_use]
    pub fn fw_inventory_wrong_release_date(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    /// `Links.ContainedBy` on a chassis carries fields beyond `@odata.id`.
    #[must_use]
    pub fn bug_invalid_contained_by_fields(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// The service root omits navigation properties that a direct GET
    /// still serves: Vikings before a BMC reset, Lite-On power shelves for
    /// `/Systems`.
    #[must_use]
    pub const fn bug_missing_root_nav_properties(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::AmiViking | Platform::Anonymous1_9_0 | Platform::LiteonPowershelf)
        )
    }

    /// A chassis omits `ChassisType`, which the schema requires.
    #[must_use]
    pub fn bug_missing_chassis_type_field(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// A chassis omits `Name`, which every resource requires.
    #[must_use]
    pub fn bug_missing_chassis_name_field(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// A DPU in NIC mode reports an empty-string `UUID` on chassis and
    /// computer system documents.
    #[must_use]
    pub fn bug_empty_uuid_field(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }

    /// A DPU serves the computer system's `Oem.Nvidia` object as a separate
    /// resource, inlining only a partially expanded stub, and puts `BaseMAC`
    /// and `Mode` in it, neither of which the NVIDIA OEM CSDL declares. Both
    /// the extra fetch and reading the two properties out of the raw body
    /// are restricted to this platform.
    #[must_use]
    pub fn bug_dpu_oem_computer_system(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }

    /// The update service omits `Name`, which every resource requires.
    #[must_use]
    pub fn bug_missing_update_service_name_field(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// `LastResetTime` on a computer system is `0000-00-00T00:00:00+00:00`,
    /// which is not a `DateTimeOffset`.
    #[must_use]
    pub fn computer_systems_wrong_last_reset_time(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    /// Event records in the SSE payload omit `MemberId`.
    #[must_use]
    pub const fn event_service_sse_no_member_id(&self) -> bool {
        matches!(self.platform, Some(Platform::Nvidia | Platform::Wiwynn))
    }

    /// Event records in the SSE payload stamp `EventTimestamp` with a
    /// compact timezone offset such as `-0600`.
    #[must_use]
    pub fn event_service_sse_wrong_timestamp_offset(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    /// Event records in the SSE payload omit `EventType`.
    #[must_use]
    pub const fn event_service_sse_missing_event_type(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::Nvidia | Platform::VeraRubin | Platform::Wiwynn)
        )
    }

    /// The SSE payload omits `@odata.id`.
    #[must_use]
    #[allow(clippy::unused_self)]
    pub const fn event_service_sse_no_odata_id(&self) -> bool {
        true
    }

    /// Vera Rubin host BMCs report composite `BootOrder` entries such as
    /// `"Boot0019: Ubuntu"` while boot option resources use the bare
    /// reference.
    #[must_use]
    pub fn vera_rubin_composite_boot_order_entries(&self) -> bool {
        self.platform == Some(Platform::VeraRubin)
    }

    /// Vikings list wrong members in the computer system collection; the
    /// filter keeps the ones that are systems.
    #[must_use]
    pub fn filter_computer_system_odata_ids(&self) -> Option<fn(&str) -> bool> {
        (self.platform == Some(Platform::AmiViking)).then_some(|odata_id| {
            odata_id.ends_with("/DGX") || odata_id.ends_with("/HGX_Baseboard_0")
        })
    }

    /// Vikings list wrong members in the manager collection; the filter
    /// keeps the ones that are managers.
    #[must_use]
    pub fn filter_manager_odata_ids(&self) -> Option<fn(&str) -> bool> {
        (self.platform == Some(Platform::AmiViking)).then_some(|odata_id| {
            odata_id.ends_with("/BMC")
                || odata_id.ends_with("/HGX_BMC_0")
                || odata_id.ends_with("/HGX_FabricManager_0")
        })
    }

    /// `$expand` responses drop required fields from embedded members while
    /// the standalone GETs are complete, so expansion is not used and each
    /// member is fetched on its own.
    #[must_use]
    pub const fn expand_is_not_working_properly(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::AmiViking | Platform::AmiGb300)
        )
    }

    /// A collection's `Members` is `null` rather than `[]`.
    #[must_use]
    pub fn bug_nullable_members(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn classified(root: &Value) -> Option<Platform> {
        BmcQuirks::classify(&RootEvidence::from_root(root)).platform
    }

    #[test]
    fn a_root_classifies_into_its_platform_class() {
        assert_eq!(
            classified(&json!({ "Vendor": "Dell" })),
            Some(Platform::Dell)
        );
        assert_eq!(classified(&json!({ "Vendor": "HPE" })), Some(Platform::Hpe));
        assert_eq!(
            classified(&json!({ "Vendor": "AMI", "RedfishVersion": "1.11.0" })),
            Some(Platform::AmiViking)
        );
        assert_eq!(
            classified(&json!({ "Vendor": "AMI", "Oem": { "Ami": { "RtpVersion": "13.09.1" } } })),
            Some(Platform::AmiGb300)
        );
        assert_eq!(classified(&json!({ "Vendor": "AMI" })), None);
        assert_eq!(
            classified(&json!({ "Vendor": "NVIDIA", "Product": "VR NVL72" })),
            Some(Platform::VeraRubin)
        );
        assert_eq!(
            classified(&json!({ "Vendor": "NVIDIA", "Product": "P3809" })),
            Some(Platform::NvSwitch)
        );
        assert_eq!(
            classified(&json!({ "Vendor": "NVIDIA", "Product": "DGX" })),
            Some(Platform::Nvidia)
        );
        assert_eq!(
            classified(&json!({ "Vendor": "Nvidia", "Product": "BlueField-3 DPU" })),
            Some(Platform::NvidiaDpu)
        );
        assert_eq!(
            classified(&json!({ "Vendor": "WIWYNN" })),
            Some(Platform::Wiwynn)
        );
        assert_eq!(
            classified(&json!({ "Vendor": "LITE-ON Technology" })),
            Some(Platform::LiteonPowershelf)
        );
        assert_eq!(
            classified(&json!({ "RedfishVersion": "1.9.0" })),
            Some(Platform::Anonymous1_9_0)
        );
        assert_eq!(classified(&json!({ "Vendor": "Contoso" })), None);
        assert_eq!(classified(&json!({})), None);
    }
}
