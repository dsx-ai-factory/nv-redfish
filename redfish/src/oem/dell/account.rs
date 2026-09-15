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

//! Dell iDRAC account-service behavior.

use crate::account::AccountServiceConfig;
#[cfg(feature = "managers")]
use crate::core::Bmc;
#[cfg(feature = "managers")]
use crate::manager::Manager;

/// Numeric iDRAC major version used to select account-management behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct IdracVersion(u32);

impl IdracVersion {
    /// iDRAC8 account behavior.
    pub const IDRAC8: Self = Self(8);
    /// iDRAC9 account behavior.
    pub const IDRAC9: Self = Self(9);
    /// iDRAC10 account behavior.
    pub const IDRAC10: Self = Self(10);

    /// Detect the iDRAC version from a Redfish Manager's Model.
    #[cfg(feature = "managers")]
    #[must_use]
    pub fn from_manager<B: Bmc>(manager: &Manager<B>) -> Option<Self> {
        let raw = manager.raw();
        let model = raw.model.as_ref()?.as_deref()?;
        Self::from_manager_model(model)
    }

    /// Detect the iDRAC version from a Manager Model such as `16G Monolithic`.
    ///
    /// Dell maps 12G/13G systems to iDRAC8, 14G through 16G to iDRAC9,
    /// and 17G or newer systems to iDRAC10.
    #[must_use]
    pub fn from_manager_model(model: &str) -> Option<Self> {
        let generation = model.split_whitespace().find_map(|part| {
            part.strip_suffix('G')
                .or_else(|| part.strip_suffix('g'))?
                .parse::<u32>()
                .ok()
        })?;

        match generation {
            12 | 13 => Some(Self::IDRAC8),
            14..=16 => Some(Self::IDRAC9),
            17.. => Some(Self::IDRAC10),
            _ => None,
        }
    }

    /// Map this iDRAC version to immutable account-service behavior.
    #[must_use]
    pub const fn account_service_config(self) -> AccountServiceConfig {
        if self.0 >= Self::IDRAC10.0 {
            AccountServiceConfig::standard()
        } else {
            AccountServiceConfig::fixed_slots(3..=16)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::IdracVersion;

    #[test]
    fn manager_model_maps_to_idrac_version() {
        assert_eq!(
            IdracVersion::from_manager_model("13G Monolithic"),
            Some(IdracVersion::IDRAC8)
        );
        assert_eq!(
            IdracVersion::from_manager_model("16G Monolithic"),
            Some(IdracVersion::IDRAC9)
        );
        assert_eq!(
            IdracVersion::from_manager_model("17G Monolithic"),
            Some(IdracVersion::IDRAC10)
        );
    }

    #[test]
    fn unknown_manager_model_is_not_assumed_to_be_idrac10() {
        assert_eq!(IdracVersion::from_manager_model("Integrated Dell"), None);
        assert_eq!(IdracVersion::from_manager_model("11G Monolithic"), None);
    }
}
