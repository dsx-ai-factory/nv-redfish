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
//! Secure boot.

use crate::computer_system::secure_boot_database::SecureBootDatabaseCollection;
use crate::schema::secure_boot::SecureBoot as SecureBootSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::ActionError;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::secure_boot::ResetKeysType as SecureBootResetKeysType;
#[doc(inline)]
pub use crate::schema::secure_boot::{SecureBootCurrentBootType, SecureBootUpdate};

/// Secure boot.
///
/// Provides functions to access Secure Boot functions.
pub struct SecureBoot<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SecureBootSchema>,
}

impl<B: Bmc> SecureBoot<B> {
    /// Create a new secure boot handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SecureBootSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for the Secure boot.
    #[must_use]
    pub fn raw(&self) -> Arc<SecureBootSchema> {
        self.data.clone()
    }

    /// Update this secure boot resource.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &SecureBootUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<SecureBootSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Get the UEFI Secure Boot database collection.
    ///
    /// Returns `Ok(None)` when this resource does not advertise databases.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the collection fails.
    pub async fn databases(&self) -> Result<Option<SecureBootDatabaseCollection<B>>, Error<B>> {
        let Some(databases) = &self.data.secure_boot_databases else {
            return Ok(None);
        };
        SecureBootDatabaseCollection::new(&self.bmc, databases)
            .await
            .map(Some)
    }

    /// Reset or delete UEFI Secure Boot keys.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is unavailable or invocation fails.
    pub async fn reset_keys(
        &self,
        reset_type: SecureBootResetKeysType,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        if actions.reset_keys.is_none() {
            return Err(Error::ActionNotAvailable);
        }
        actions
            .reset_keys(self.bmc.as_ref(), reset_type)
            .await
            .map_err(Error::from)
    }

    /// Get an indication of whether UEFI Secure Boot is enabled.
    #[must_use]
    pub fn secure_boot_enable(&self) -> Option<bool> {
        self.data.secure_boot_enable.and_then(identity)
    }

    /// The UEFI Secure Boot state during the current boot cycle.
    #[must_use]
    pub fn secure_boot_current_boot(&self) -> Option<SecureBootCurrentBootType> {
        self.data.secure_boot_current_boot.and_then(identity)
    }
}
