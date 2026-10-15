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

//! UEFI Secure Boot databases.

use std::sync::Arc;

use crate::certificate::CertificateCollection;
use crate::schema::secure_boot_database::SecureBootDatabase as SecureBootDatabaseSchema;
use crate::schema::secure_boot_database_collection::SecureBootDatabaseCollection as SecureBootDatabaseCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::ActionError;
use nv_redfish_core::Bmc;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;

#[doc(inline)]
pub use crate::schema::secure_boot_database::ResetKeysType as SecureBootDatabaseResetKeysType;

/// Collection of UEFI Secure Boot databases.
pub struct SecureBootDatabaseCollection<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SecureBootDatabaseCollectionSchema>,
}

impl<B: Bmc> SecureBootDatabaseCollection<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SecureBootDatabaseCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// List the databases in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching a database fails.
    pub async fn members(&self) -> Result<Vec<SecureBootDatabase<B>>, Error<B>> {
        let mut members = Vec::with_capacity(self.data.members.len());
        for member in &self.data.members {
            members.push(SecureBootDatabase::new(&self.bmc, member).await?);
        }
        Ok(members)
    }

    /// Get the raw schema data for this collection.
    #[must_use]
    pub fn raw(&self) -> Arc<SecureBootDatabaseCollectionSchema> {
        self.data.clone()
    }
}

/// One UEFI Secure Boot database.
pub struct SecureBootDatabase<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SecureBootDatabaseSchema>,
}

impl<B: Bmc> SecureBootDatabase<B> {
    async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SecureBootDatabaseSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get this database's certificate collection.
    ///
    /// Returns `Ok(None)` when the database does not advertise certificates.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the collection fails.
    pub async fn certificates(&self) -> Result<Option<CertificateCollection<B>>, Error<B>> {
        let Some(certificates) = &self.data.certificates else {
            return Ok(None);
        };
        CertificateCollection::new(&self.bmc, certificates)
            .await
            .map(Some)
    }

    /// Reset or delete this database's keys.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is unavailable or invocation fails.
    pub async fn reset_keys(
        &self,
        reset_type: SecureBootDatabaseResetKeysType,
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

    /// Get the raw schema data for this database.
    #[must_use]
    pub fn raw(&self) -> Arc<SecureBootDatabaseSchema> {
        self.data.clone()
    }
}
