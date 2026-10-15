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

use crate::schema::service_root::ServiceRoot;

/// Object that provides quirks of individual platforms. On first root
/// retrieval we classify platform and then apply specific workarounds
/// for each individual platform class.
pub struct BmcQuirks {
    platform: Option<Platform>,
}

// Platform shouldn't be considered as vendor. Actually it is class of
// devices that have the same set of quirks.
#[derive(PartialEq, Eq)]
enum Platform {
    Hpe,
    Dell,
    Ami,
    AmiViking,
    AmiGb300,
    LenovoAmi,
    VeraRubin,
    Nvidia,
    NvidiaDpu,
    Wiwynn,
    Anonymous1_9_0,
    LiteonPowershelf,
    NvSwitch,
    Supermicro,
}

impl BmcQuirks {
    pub fn new(root: &ServiceRoot) -> Self {
        let vendor_str = root.vendor.as_ref().and_then(Option::as_deref);
        let redfish_version_str = root.redfish_version.as_deref();
        let product_str = root.product.as_ref().and_then(Option::as_deref);
        // The GB300 host BMC exposes an AMI OEM `RtpVersion` in the service
        // root; use it to distinguish GB300 from other AMI BMCs so the
        // expand workaround is not applied to every AMI platform.
        let ami_oem = root
            .oem
            .as_ref()
            .and_then(|oem| oem.additional_properties.get("Ami"));
        let rtp_version = ami_oem
            .and_then(|ami| ami.get("RtpVersion"))
            .and_then(|v| v.as_str());
        let platform = match vendor_str {
            Some("HPE") => Some(Platform::Hpe),
            Some("Dell") => Some(Platform::Dell),
            Some("AMI") if redfish_version_str == Some("1.11.0") => Some(Platform::AmiViking),
            Some("AMI") if rtp_version == Some("13.09.1") => Some(Platform::AmiGb300),
            Some("AMI") => Some(Platform::Ami),
            // Lenovo trays running AMI firmware (HS350x class) keep the
            // Lenovo vendor but expose an AMI OEM object in the service root.
            Some("Lenovo") if ami_oem.is_some() => Some(Platform::LenovoAmi),
            Some("NVIDIA") if product_str == Some("VR NVL72") => Some(Platform::VeraRubin),
            Some("NVIDIA") if product_str == Some("P3809") => Some(Platform::NvSwitch),
            Some("NVIDIA") => Some(Platform::Nvidia),
            // BlueField DPU service roots use an `Nvidia` vendor with these
            // product names; BlueField-4 reports `BlueField-4`.
            Some("Nvidia")
                if matches!(
                    product_str,
                    Some("Nvidia-BMCMezz" | "BlueField-3 DPU" | "BlueField-4")
                ) =>
            {
                Some(Platform::NvidiaDpu)
            }
            // Wiwynn ODM GB200 NVL trays report their own vendor rather than
            // `NVIDIA`
            Some("WIWYNN") => Some(Platform::Wiwynn),
            Some("Supermicro") => Some(Platform::Supermicro),
            Some(vendor) if vendor.starts_with("LITE-ON") => Some(Platform::LiteonPowershelf),
            None if redfish_version_str == Some("1.9.0") => Some(Platform::Anonymous1_9_0),
            _ => None,
        };
        Self { platform }
    }

    /// NVIDIA platforms can report the unfiltered count for filtered log collections.
    #[cfg(feature = "log-services")]
    pub(crate) const fn bug_filtered_log_entries_count(&self) -> bool {
        matches!(self.platform, Some(Platform::Nvidia | Platform::VeraRubin))
    }

    // Account type is required according to schema specification
    // (marked with Redfish.Required annotation) but some vendors
    // ignores this flag. A workaround for this bug is supported by
    // `nv-redfish`.
    #[cfg(feature = "accounts")]
    pub(crate) fn bug_no_account_type_in_accounts(&self) -> bool {
        self.platform == Some(Platform::Hpe)
    }

    /// NVIDIA DPUs inline an incomplete AccountService certificate collection.
    #[cfg(feature = "accounts")]
    pub(crate) fn bug_incomplete_account_service_certificate_collection(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }

    // In some implementations BMC ReleaseDate is incorrectly set to
    // 00:00:00Z in FirmwareInventory (which is
    // SoftwareInventoryCollection).
    #[cfg(feature = "update-service")]
    pub(crate) fn fw_inventory_wrong_release_date(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    // iDRAC answers a Volume create with 200 or 201 and the URI of the
    // Dell job that will build the volume (in Location or as the body's
    // `@odata.id`) instead of 202 Accepted.
    #[cfg(feature = "storages")]
    pub(crate) fn bug_job_location_in_create_response(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    /// In some cases there is addtional fields in Links.ContainedBy in
    /// Chassis resource, this flag aims to patch this invalid links
    #[cfg(feature = "chassis")]
    pub(crate) fn bug_invalid_contained_by_fields(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// Missing navigation properties in root object.
    #[cfg(any(
        feature = "chassis",
        feature = "computer-systems",
        feature = "managers",
        feature = "update-service",
    ))]
    pub(crate) const fn bug_missing_root_nav_properties(&self) -> bool {
        match self.platform {
            // 1. There are situations when Viking doesn't provide any
            //    navigation properties in root before BMC reset.
            // 2. Lite-On power shelves omit Systems on the service root while
            //    `/Systems` is still available via direct GET.
            Some(Platform::AmiViking | Platform::Anonymous1_9_0 | Platform::LiteonPowershelf) => {
                true
            }
            _ => false,
        }
    }

    /// Missing chassis type property in Chassis resource. This
    /// property is Required in according to specification but some
    /// systems doesn't provide it.
    #[cfg(feature = "chassis")]
    pub(crate) fn bug_missing_chassis_type_field(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// Missing Name property in Chassis resource. This property is
    /// required in any resource.
    #[cfg(feature = "chassis")]
    pub(crate) fn bug_missing_chassis_name_field(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// NVIDIA DPU sometimes returns empty string UUID in
    /// chassis/computer system payloads when DPU is in NIC mode.
    #[cfg(any(feature = "chassis", feature = "computer-systems"))]
    pub(crate) fn bug_empty_uuid_field(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }

    /// NVIDIA DPU serves the computer system `Oem.Nvidia` object as a
    /// separate resource, inlining only a partially expanded stub, and
    /// carries properties and actions described by our local CSDL
    /// additions. The extra fetch is restricted to this platform.
    #[cfg(all(feature = "computer-systems", feature = "oem-nvidia"))]
    pub(crate) fn bug_dpu_oem_computer_system(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }

    /// Missing Name property in Chassis resource. This property is
    /// required in any resource.
    #[cfg(feature = "update-service")]
    pub(crate) fn bug_missing_update_service_name_field(&self) -> bool {
        self.platform == Some(Platform::AmiViking)
    }

    /// H100 Viking reports the standard `PEMchain` certificate type as
    /// `PEMChain`.
    #[cfg(feature = "component-integrity")]
    pub(crate) const fn certificate_type_wrong_pem_chain_case(&self) -> bool {
        matches!(self.platform, Some(Platform::AmiViking))
    }

    /// In some implementations BMC ReleaseDate is incorrectly set to
    /// "0000-00-00T00:00:00+00:00" in ComputerSystem/LastResetTime
    /// This prevents ComputerSystem to be correctly parsed because
    /// this is invalid Edm.DateTimeOffset.
    #[cfg(feature = "computer-systems")]
    pub(crate) fn computer_systems_wrong_last_reset_time(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    /// In some implementations, Event records in SSE payload do not include
    /// `MemberId`.
    #[cfg(feature = "event-service")]
    pub(crate) const fn event_service_sse_no_member_id(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::Nvidia | Platform::VeraRubin | Platform::NvSwitch | Platform::Wiwynn)
        )
    }

    /// Event records can contain an empty object instead of a `LogEntry` reference.
    #[cfg(feature = "event-service")]
    pub(crate) fn event_service_sse_empty_log_entry(&self) -> bool {
        self.platform == Some(Platform::VeraRubin)
    }

    /// In some implementations, Event records in SSE payload use compact
    /// timezone offsets in `EventTimestamp` (for example, `-0600`).
    #[cfg(feature = "event-service")]
    pub(crate) fn event_service_sse_wrong_timestamp_offset(&self) -> bool {
        self.platform == Some(Platform::Dell)
    }

    /// In some implementations, Event records in SSE payload omit `EventType`.
    #[cfg(feature = "event-service")]
    pub(crate) const fn event_service_sse_missing_event_type(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::Nvidia | Platform::VeraRubin | Platform::NvSwitch | Platform::Wiwynn)
        )
    }

    /// SSE payload does not include `@odata.id`.
    #[cfg(feature = "event-service")]
    #[allow(clippy::unused_self)]
    pub(crate) const fn event_service_sse_no_odata_id(&self) -> bool {
        true
    }

    /// Event envelopes can omit both `Id` and `@odata.id` while carrying an SSE ID.
    #[cfg(feature = "event-service")]
    pub(crate) const fn event_service_sse_missing_envelope_ids(&self) -> bool {
        matches!(self.platform, Some(Platform::VeraRubin))
    }

    /// Vera Rubin host BMCs report composite `BootOrder` entries such as
    /// `"Boot0019: Ubuntu"` while boot option resources use the bare reference.
    #[cfg(feature = "computer-systems")]
    pub(crate) fn vera_rubin_composite_boot_order_entries(&self) -> bool {
        self.platform == Some(Platform::VeraRubin)
    }

    /// Vikings provide wrong elements in computer system
    /// collection. This function returns ODataId filter function for
    /// these collections.
    #[cfg(feature = "computer-systems")]
    pub(crate) fn filter_computer_system_odata_ids(&self) -> Option<fn(&str) -> bool> {
        (self.platform == Some(Platform::AmiViking)).then_some(|odata_id| {
            odata_id.ends_with("/DGX") || odata_id.ends_with("/HGX_Baseboard_0")
        })
    }

    /// Vikings provide wrong elements in manager
    /// collection. This function returns ODataId filter function for
    /// these collections.
    #[cfg(feature = "managers")]
    pub(crate) fn filter_manager_odata_ids(&self) -> Option<fn(&str) -> bool> {
        (self.platform == Some(Platform::AmiViking)).then_some(|odata_id| {
            odata_id.ends_with("/BMC")
                || odata_id.ends_with("/HGX_BMC_0")
                || odata_id.ends_with("/HGX_FabricManager_0")
        })
    }

    /// Lite-On power shelves advertise `ResetType` for
    /// `Manager.ResetToDefaults` but reject it, requiring the parameter
    /// to be named `ResetToDefaultsType`.
    ///
    /// Their service root often omits `Vendor` and is then indistinguishable
    /// from a Delta shelf, so the manager's own `Manufacturer` is checked too.
    #[cfg(feature = "managers")]
    pub(crate) fn bug_reset_to_defaults_type_parameter(
        &self,
        manager_manufacturer: Option<&str>,
    ) -> bool {
        self.platform == Some(Platform::LiteonPowershelf)
            || manager_manufacturer.is_some_and(|manufacturer| {
                manufacturer.eq_ignore_ascii_case("LITE-ON TECHNOLOGY CORP.")
            })
    }

    /// Lite-On PMC 2.3.3+ reports an LED-state object instead of a boolean
    /// in PowerSupply.LocationIndicatorActive. Some shelves omit Vendor in
    /// the service root, so also check the chassis manufacturer.
    #[cfg(feature = "power-supplies")]
    pub(crate) fn bug_power_supply_location_indicator_active(
        &self,
        chassis_manufacturer: Option<&str>,
    ) -> bool {
        self.platform == Some(Platform::LiteonPowershelf)
            || chassis_manufacturer.is_some_and(|manufacturer| {
                manufacturer.eq_ignore_ascii_case("LITE-ON TECHNOLOGY CORP.")
            })
    }

    /// In some cases we expand is not working according to spec,
    /// if it is the case for specific chassis, we would disable
    /// expand api.
    ///
    /// AMI host BMCs (Viking and the GB300-class `Ami`) return `$expand`
    /// responses that drop Required fields (Id/Name/ChassisType) from embedded
    /// members; the standalone resource GETs are complete, so disabling expand
    /// makes nv-redfish fetch each member individually and parse correctly.
    pub(crate) const fn expand_is_not_working_properly(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::AmiViking | Platform::AmiGb300)
        )
    }

    /// Some implementations return the `Members` field of
    /// a collection as `null` instead of an empty array (`[]`).
    #[cfg(feature = "patch-collection")]
    pub(crate) fn bug_nullable_members(&self) -> bool {
        self.platform == Some(Platform::NvidiaDpu)
    }

    /// AMI MegaRAC firmware requires `If-Match` on PATCH but rejects the
    /// ETag it served, accepting only `*`.
    pub(crate) const fn bug_rejects_own_etag(&self) -> bool {
        matches!(
            self.platform,
            Some(Platform::Ami | Platform::AmiViking | Platform::AmiGb300 | Platform::LenovoAmi)
        )
    }

    /// Supermicro answers a staged BIOS settings change with `202 Accepted`
    /// and nothing to poll: no `Location` and no Task.
    pub(crate) const fn bug_accepted_update_without_task(&self) -> bool {
        matches!(self.platform, Some(Platform::Supermicro))
    }
}

#[cfg(test)]
#[cfg(feature = "event-service")]
mod tests {
    use super::*;

    #[test]
    fn sse_repairs_are_scoped_to_affected_platforms() {
        for (platform, missing_member_id, empty_log_entry, missing_envelope_ids) in [
            (Some(Platform::VeraRubin), true, true, true),
            (Some(Platform::NvSwitch), true, false, false),
            (Some(Platform::Nvidia), true, false, false),
            (Some(Platform::Wiwynn), true, false, false),
            (Some(Platform::Dell), false, false, false),
            (Some(Platform::Hpe), false, false, false),
            (Some(Platform::NvidiaDpu), false, false, false),
            (None, false, false, false),
        ] {
            let quirks = BmcQuirks { platform };

            assert_eq!(quirks.event_service_sse_no_member_id(), missing_member_id);
            assert_eq!(quirks.event_service_sse_empty_log_entry(), empty_log_entry);

            assert_eq!(
                quirks.event_service_sse_missing_envelope_ids(),
                missing_envelope_ids
            );
        }
    }
}
