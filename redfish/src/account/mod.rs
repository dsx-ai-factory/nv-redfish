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

//! AccountService (Redfish) — high-level wrappers
//!
//! Feature: `accounts` (this module is compiled only when the feature is enabled).
//!
//! This module provides ergonomic wrappers around the generated Redfish
//! AccountService model:
//! - `AccountService`: entry point to manage accounts
//! - `AccountCollection`: access and create `ManagerAccount` members
//! - `Account`: operate on an individual `ManagerAccount`
//!
//! Vendor compatibility
//! - Some implementations omit fields marked as `Redfish.Required`.
//! - Every read goes through the compatibility layer of `nv-redfish-quirks`,
//!   which repairs such documents for the platform (for example, defaulting
//!   `AccountTypes`).
//!

/// Collection of accounts.
mod collection;
/// Account inside account service.
mod item;

use crate::schema::account_service::AccountService as SchemaAccountService;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::ops::RangeInclusive;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::account_service::AccountServiceUpdate;
#[doc(inline)]
pub use crate::schema::manager_account::AccountTypes;
#[doc(inline)]
pub use crate::schema::manager_account::ManagerAccountCreate;
#[doc(inline)]
pub use crate::schema::manager_account::ManagerAccountUpdate;
#[doc(inline)]
pub use item::Account;

#[doc(inline)]
pub use collection::AccountCollection;
#[doc(inline)]
pub(crate) use collection::FixedSlotConfig;
pub(crate) use item::AccountReply;
#[doc(inline)]
pub(crate) use item::Config as AccountConfig;
pub(crate) use item::DeletionStrategy;

/// Immutable account behavior selected when loading [`AccountService`].
#[derive(Clone)]
pub struct AccountServiceConfig {
    behavior: AccountServiceBehavior,
}

#[derive(Clone)]
enum AccountServiceBehavior {
    Standard,
    FixedSlots(RangeInclusive<u32>),
}

impl AccountServiceConfig {
    /// Use standard Redfish collection `POST` and resource `DELETE` semantics.
    #[must_use]
    pub const fn standard() -> Self {
        Self {
            behavior: AccountServiceBehavior::Standard,
        }
    }

    #[allow(dead_code)] // Constructed by OEM behavior mappings when enabled.
    pub(crate) const fn fixed_slots(slots: RangeInclusive<u32>) -> Self {
        Self {
            behavior: AccountServiceBehavior::FixedSlots(slots),
        }
    }

    fn collection_config(&self) -> collection::Config {
        match &self.behavior {
            AccountServiceBehavior::Standard => collection::Config {
                account: AccountConfig {
                    deletion_strategy: DeletionStrategy::DeleteResource,
                },
                fixed_slots: None,
            },
            AccountServiceBehavior::FixedSlots(slots) => collection::Config {
                account: AccountConfig {
                    deletion_strategy: DeletionStrategy::DisableSlot,
                },
                fixed_slots: Some(FixedSlotConfig {
                    slots: slots.clone(),
                }),
            },
        }
    }
}

/// Account service. Provides the ability to manage accounts via Redfish.
pub struct AccountService<B: Bmc> {
    config: AccountServiceConfig,
    service: Arc<SchemaAccountService>,
    bmc: NvBmc<B>,
}

impl<B: Bmc> AccountService<B> {
    /// Create a new account service. This is always done by
    /// `ServiceRoot` object.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
        config: AccountServiceConfig,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(service_nav) = root.root.account_service.as_ref() else {
            return Ok(None);
        };
        let service = service_nav.get(bmc.as_ref()).await.map_err(Error::from)?;
        Ok(Some(Self {
            config,
            service,
            bmc: bmc.clone(),
        }))
    }

    /// Get the raw schema data for this account service.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<SchemaAccountService> {
        self.service.clone()
    }

    /// Update this account service.
    ///
    /// The immutable account behavior is retained when the response contains
    /// an updated resource.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &AccountServiceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<SchemaAccountService>>(
                self.service.odata_id(),
                self.service.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move {
                let service = nav.get(self.bmc.as_ref()).await.map_err(Error::from)?;
                Ok(Self {
                    config: self.config.clone(),
                    service,
                    bmc: self.bmc.clone(),
                })
            })
            .await
    }

    /// Get the accounts collection.
    ///
    /// Uses `$expand` to retrieve members in a single request when supported.
    /// Creation, listing, and deletion use the immutable behavior selected by
    /// the [`AccountServiceConfig`] passed to
    /// [`ServiceRoot::account_service`](crate::ServiceRoot::account_service).
    ///
    /// # Errors
    ///
    /// Returns an error if expanding the collection fails.
    pub async fn accounts(&self) -> Result<Option<AccountCollection<B>>, Error<B>> {
        if let Some(collection_ref) = self.service.accounts.as_ref() {
            let config = self.config.collection_config();
            AccountCollection::new(self.bmc.clone(), collection_ref, config)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }
}
