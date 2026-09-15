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

//! Accounts collection utilities.
//!
//! Provides `AccountCollection` for working with the Redfish
//! `ManagerAccountCollection`.
//!
//! - List members and fetch full account data without mutating the
//!   collection via `all_accounts_data`.
//! - Create accounts:
//!   - Default: create a new `ManagerAccount` resource.
//!   - Slot-defined mode: reuse the lowest available disabled slot in the
//!     configured inclusive range.
//!
//! Configuration:
//! - `account`: controls read patching via `read_patch_fn`.
//! - `fixed_slots`: inclusive numeric slot range. Disabled slots are omitted
//!   from `all_accounts_data`.
//!
//! Other:
//! - `odata_id()` returns the collection `@odata.id` (typically
//!   `/redfish/v1/AccountService/Accounts`).
//! - Collection reads use `$expand` with depth 1 to materialize
//!   members when available.

use crate::account::Account;
use crate::account::AccountConfig;
use crate::account::ManagerAccountCreate;
use crate::account::ManagerAccountUpdate;
use crate::patch_support::CollectionWithPatch;
use crate::patch_support::CreateWithPatch;
use crate::patch_support::ReadPatchFn;
use crate::schema::manager_account::ManagerAccount;
use crate::schema::manager_account_collection::ManagerAccountCollection;
use crate::schema::resource::ResourceCollection;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use std::ops::RangeInclusive;
use std::sync::Arc;

/// Configuration for slot-defined user accounts.
///
/// In slot-defined mode, accounts are pre-provisioned as numeric-id "slots".
/// Creation reuses the lowest eligible disabled slot in `slots`, listing hides
/// disabled slots, and deletion disables instead of removing the slot.
#[derive(Clone)]
pub struct FixedSlotConfig {
    /// Inclusive slot range. Each slot is identified by a numeric resource
    /// `Id`.
    pub slots: RangeInclusive<u32>,
}

/// Configuration for account collection behavior.
///
/// Combines per-account settings and optional slot-defined mode that changes
/// how accounts are created, listed, and deleted.
#[derive(Clone)]
pub struct Config {
    /// Configuration of `Account` objects.
    pub account: AccountConfig,
    /// Configuration for fixed preallocated account slots.
    pub fixed_slots: Option<FixedSlotConfig>,
}

/// Account collection.
///
/// Provides functions to access collection members.
pub struct AccountCollection<B: Bmc> {
    config: Config,
    bmc: NvBmc<B>,
    collection: Arc<ManagerAccountCollection>,
}

impl<B: Bmc> CollectionWithPatch<ManagerAccountCollection, ManagerAccount, B>
    for AccountCollection<B>
{
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<ManagerAccount>>,
    ) -> ManagerAccountCollection {
        ManagerAccountCollection { base, members }
    }
}

impl<B: Bmc> CreateWithPatch<ManagerAccountCollection, ManagerAccount, ManagerAccountCreate, B>
    for AccountCollection<B>
{
    fn entity_ref(&self) -> &ManagerAccountCollection {
        self.collection.as_ref()
    }
    fn patch(&self) -> Option<&ReadPatchFn> {
        self.config.account.read_patch_fn.as_ref()
    }
    fn bmc(&self) -> &B {
        self.bmc.as_ref()
    }
}

impl<B: Bmc> AccountCollection<B> {
    pub(crate) async fn new(
        bmc: NvBmc<B>,
        collection_ref: &NavProperty<ManagerAccountCollection>,
        config: Config,
    ) -> Result<Self, Error<B>> {
        let collection = Self::expand_collection(
            &bmc,
            collection_ref,
            config.account.read_patch_fn.as_ref(),
            None,
        )
        .await?;
        Ok(Self {
            config,
            bmc,
            collection,
        })
    }

    /// `OData` identifier of the account collection in Redfish.
    ///
    /// Typically `/redfish/v1/AccountService/Accounts`.
    #[must_use]
    pub fn odata_id(&self) -> &ODataId {
        self.collection.as_ref().odata_id()
    }

    /// Create a new account.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the newly created account.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if creating a new account fails.
    pub async fn create_account(
        &self,
        create: ManagerAccountCreate,
    ) -> Result<ModificationResponse<Account<B>>, Error<B>> {
        if let Some(cfg) = &self.config.fixed_slots {
            // Collection order is not a stable allocation policy. Fetch every
            // member first, then consider disabled numeric slots in ascending
            // order within the configured bounds.
            let mut candidates = Vec::new();
            for nav in &self.collection.members {
                let account = Account::new(&self.bmc, nav, &self.config.account).await?;
                let Ok(id) = account.raw().base.id.parse::<u32>() else {
                    continue;
                };
                if !cfg.slots.contains(&id) || account.is_enabled() {
                    continue;
                }
                candidates.push((id, account));
            }
            candidates.sort_unstable_by_key(|(id, _)| *id);

            for (_, account) in candidates {
                // Expanded collection members are a snapshot. Re-fetch the
                // candidate immediately before updating it so concurrent
                // account creation cannot reuse stale slot state. Require a
                // fresh ETag so the HTTP BMC cannot fall back to `If-Match: *`.
                let account = Account::new(
                    &self.bmc,
                    &NavProperty::new_reference(account.raw().odata_id().clone()),
                    &self.config.account,
                )
                .await?;
                if account.is_enabled() || account.raw().etag().is_none() {
                    continue;
                }

                // Build an update based on the create request:
                let update = ManagerAccountUpdate {
                    base: None,
                    user_name: Some(create.user_name),
                    password: Some(create.password),
                    role_id: Some(create.role_id),
                    enabled: Some(true),
                    account_expiration: create.account_expiration,
                    account_types: create.account_types,
                    email_address: create.email_address,
                    locked: create.locked,
                    oem_account_types: create.oem_account_types,
                    one_time_passcode_delivery_address: create.one_time_passcode_delivery_address,
                    password_change_required: create.password_change_required,
                    password_expiration: create.password_expiration,
                    phone_number: create.phone_number,
                    snmp: create.snmp,
                    strict_account_types: create.strict_account_types,
                    mfa_bypass: create.mfa_bypass,
                    links: None,
                };

                return account.update(&update).await;
            }
            // No available slot found
            Err(Error::AccountSlotNotAvailable)
        } else {
            Ok(self
                .create_with_patch(&create)
                .await?
                .map_entity(|account| {
                    Account::from_data(self.bmc.clone(), account, self.config.account.clone())
                }))
        }
    }

    /// Retrieve account data.
    ///
    /// This method does not update the collection itself. It only
    /// retrieves all account data (if not already retrieved).
    ///
    /// # Errors
    ///
    /// Returns an error if retrieving account data fails. This can
    /// occur if the account collection was not expanded.
    pub async fn all_accounts_data(&self) -> Result<Vec<Account<B>>, Error<B>> {
        let mut result = Vec::with_capacity(self.collection.members.len());
        if self.config.fixed_slots.is_some() {
            // Disabled fixed slots are implementation details, not accounts
            // visible to callers.
            for m in &self.collection.members {
                let account = Account::new(&self.bmc, m, &self.config.account).await?;
                if account.is_enabled() {
                    result.push(account);
                }
            }
        } else {
            for m in &self.collection.members {
                result.push(Account::new(&self.bmc, m, &self.config.account).await?);
            }
        }
        Ok(result)
    }
}
