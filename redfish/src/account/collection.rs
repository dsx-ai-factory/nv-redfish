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
//!   - Slot-defined mode: reuse the first available disabled slot,
//!     honoring `min_slot` when configured.
//!
//! Configuration:
//! - `account`: controls read patching via `read_patch_fn`.
//! - `slot_defined_user_accounts`:
//!   - `min_slot`: minimum numeric slot id considered.
//!   - `hide_disabled`: omit disabled accounts from `all_accounts_data`.
//!   - `disable_account_on_delete`: prefer disabling over deletion.
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
use std::sync::Arc;

/// Configuration for slot-defined user accounts.
///
/// In slot-defined mode, accounts are pre-provisioned as numeric-id "slots".
/// Creation reuses the first eligible disabled slot (respecting `min_slot`),
/// listing may hide disabled slots, and deletion can disable instead of remove.
#[derive(Clone)]
pub struct SlotDefinedConfig {
    /// Minimum slot number (the slot is identified by an `Id`
    /// containing a numeric string).
    pub min_slot: Option<u32>,
    /// Hide disabled accounts when listing all accounts.
    pub hide_disabled: bool,
    /// Disable the account instead of deleting it.
    pub disable_account_on_delete: bool,
}

/// Configuration for account collection behavior.
///
/// Combines per-account settings and optional slot-defined mode that changes
/// how accounts are created, listed, and deleted.
#[derive(Clone)]
pub struct Config {
    /// Configuration of `Account` objects.
    pub account: AccountConfig,
    /// Configuration for slot-defined user accounts.
    pub slot_defined_user_accounts: Option<SlotDefinedConfig>,
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
        if let Some(cfg) = &self.config.slot_defined_user_accounts {
            // For slot-defined configuration, find the first account
            // that is disabled (and whose id is >= `min_slot`, if defined)
            // and apply an update to it.
            for nav in &self.collection.members {
                let Ok(account) = Account::new(&self.bmc, nav, &self.config.account).await else {
                    continue;
                };
                if let Some(min) = cfg.min_slot {
                    // If the minimum id is configured and this slot id is below
                    // the threshold, look for another slot.
                    let Ok(id) = account.raw().base.id.parse::<u32>() else {
                        continue;
                    };
                    if id < min {
                        continue;
                    }
                }
                if account.is_enabled() {
                    // Slot is already explicitly enabled. Find another slot.
                    continue;
                }

                // Expanded collection members are a snapshot. Re-fetch the
                // candidate immediately before updating it so concurrent
                // account creation cannot reuse stale slot state.
                let fresh_nav = NavProperty::new_reference(account.raw().odata_id().clone());
                let Ok(account) = Account::new(&self.bmc, &fresh_nav, &self.config.account).await
                else {
                    continue;
                };
                if account.is_enabled() || account.raw().etag().is_none() {
                    // Without a fresh ETag, the HTTP BMC would fall back to an
                    // unconditional `If-Match: *` update.
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
        if let Some(cfg) = &self.config.slot_defined_user_accounts {
            // For slot-defined account configuration, disabled accounts may be hidden
            // to make it appear as if they were not created. This behavior is
            // controlled by the `hide_disabled` configuration parameter.
            for m in &self.collection.members {
                let account = Account::new(&self.bmc, m, &self.config.account).await?;
                if !cfg.hide_disabled || account.is_enabled() {
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
