// SPDX-FileCopyrightText: Copyright (c) 2026 NVIDIA CORPORATION & AFFILIATES. All rights reserved.
// SPDX-License-Identifier: Apache-2.0

//! Dell iDRAC account-service behavior.

use crate::account::AccountServiceConfig;

/// Dell iDRAC generation used to select account-management behavior.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IdracVersion {
    /// iDRAC9 uses preallocated account slots 3 through 16.
    Idrac9,
    /// iDRAC10 supports standard Redfish account creation and deletion.
    Idrac10,
    /// An unrecognized iDRAC generation.
    Unknown,
}

impl IdracVersion {
    /// Map this iDRAC generation to an account-service configuration.
    ///
    /// Returns `None` when the generation is unknown so callers cannot
    /// accidentally choose an incompatible account workflow.
    #[must_use]
    pub const fn account_service_config(self) -> Option<AccountServiceConfig> {
        match self {
            Self::Idrac9 => Some(AccountServiceConfig::fixed_slots(3..=16)),
            Self::Idrac10 => Some(AccountServiceConfig::standard()),
            Self::Unknown => None,
        }
    }
}
