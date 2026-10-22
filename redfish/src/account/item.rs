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

//! Redfish ManagerAccount — high-level wrapper
//!
//! Provides `Account`, an ergonomic handle over a Redfish `ManagerAccount`:
//! - Read raw data with `raw()`
//! - Update fields via `update()`, or use helpers `update_password()` and
//!   `update_user_name()`
//! - Delete the account with `delete()`
//!
//! Configuration:
//! - `Config::deletion_strategy`: delete a normal resource or safely disable a
//!   preallocated slot
//!
//! Note: `Account` objects are created by higher-level APIs (e.g.
//! `AccountCollection`) and do not create accounts on the BMC by themselves.
//! Use the collection to create new accounts.

use crate::account::ManagerAccountUpdate;
use crate::schema::manager_account::ManagerAccount;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value as JsonValue;
use std::convert::identity;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub enum DeletionStrategy {
    DeleteResource,
    DisableSlot,
}

#[derive(Clone)]
pub struct Config {
    /// How account deletion is performed.
    pub deletion_strategy: DeletionStrategy,
}

/// What an account write answers with when it is a document, kept as it
/// came so that the account is repaired before it is read. A success
/// message or a bare reference is not one, so the transport reads it as it
/// would for an account.
pub struct AccountReply(JsonValue);

impl AccountReply {
    /// The account, repaired as a read through the compatibility layer
    /// repairs it.
    pub fn into_account<B: Bmc>(self, bmc: &NvBmc<B>) -> Result<ManagerAccount, Error<B>> {
        bmc.as_ref().decode(self.0).map_err(Error::Decode)
    }
}

impl<'de> Deserialize<'de> for AccountReply {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let reply = JsonValue::deserialize(deserializer)?;
        let is_document = reply
            .as_object()
            .is_some_and(|object| object.contains_key("@odata.id") && object.len() > 1);
        if is_document {
            Ok(Self(reply))
        } else {
            Err(D::Error::custom("the reply is not an account"))
        }
    }
}

/// Represents a Redfish `ManagerAccount`.
pub struct Account<B: Bmc> {
    config: Config,
    bmc: NvBmc<B>,
    data: Arc<ManagerAccount>,
}

impl<B: Bmc> Account<B> {
    /// Create a new account handle. This does not create an account on the
    /// BMC.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ManagerAccount>,
        config: &Config,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
                config: config.clone(),
            })
    }

    /// Create from existing data.
    pub(crate) fn from_data(bmc: NvBmc<B>, data: ManagerAccount, config: Config) -> Self {
        Self {
            bmc,
            data: Arc::new(data),
            config,
        }
    }

    /// Raw `ManagerAccount` data.
    #[must_use]
    pub fn raw(&self) -> Arc<ManagerAccount> {
        self.data.clone()
    }

    /// Account is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.data.enabled.is_none_or(identity)
    }

    /// Update the account.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated account.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if the server responds with an error or if the
    /// response cannot be parsed.
    pub async fn update(
        &self,
        update: &ManagerAccountUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        let id = self.data.odata_id();
        let etag = self.data.etag();
        let updated = if self.bmc.quirks.bug_no_account_type_in_accounts() {
            // The platform answers with an account that lacks
            // `AccountTypes`: the reply is repaired as a read is.
            self.bmc
                .update::<_, AccountReply>(id, etag, update)
                .await?
                .try_map_entity(|reply| reply.into_account(&self.bmc))?
        } else {
            self.bmc
                .update::<_, ManagerAccount>(id, etag, update)
                .await?
        };
        Ok(updated.map_entity(|ma| Self::from_data(self.bmc.clone(), ma, self.config.clone())))
    }

    /// Update the account's password.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated account.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if the server responds with an error or if the
    /// response cannot be parsed.
    pub async fn update_password(
        &self,
        password: String,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update(
            &ManagerAccountUpdate::builder()
                .with_password(password)
                .build(),
        )
        .await
    }

    /// Update the account's user name.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated account.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if the server responds with an error or if the
    /// response cannot be parsed.
    pub async fn update_user_name(
        &self,
        user_name: String,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update(
            &ManagerAccountUpdate::builder()
                .with_user_name(user_name)
                .build(),
        )
        .await
    }

    /// Delete the current account.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the account returned by the
    ///   server. When deletion is configured to disable the account, this is the
    ///   updated account.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if deletion fails.
    pub async fn delete(&self) -> Result<ModificationResponse<Self>, Error<B>> {
        match self.config.deletion_strategy {
            DeletionStrategy::DeleteResource => {
                self.bmc
                    .as_ref()
                    .delete::<NavProperty<ManagerAccount>>(self.data.odata_id())
                    .await
                    .map_err(Error::from)?
                    .try_map_entity_async(|nav| async move {
                        Self::new(&self.bmc, &nav, &self.config).await
                    })
                    .await
            }
            DeletionStrategy::DisableSlot => {
                self.update(&ManagerAccountUpdate::builder().with_enabled(false).build())
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AccountReply;
    use serde_json::json;
    use serde_json::Value as JsonValue;

    #[test]
    fn only_a_document_is_an_account_reply() {
        let read = |reply: JsonValue| serde_json::from_value::<AccountReply>(reply).is_ok();
        assert!(read(json!({
            "@odata.id": "/redfish/v1/AccountService/Accounts/1",
            "UserName": "user",
        })));
        // Neither a success message nor a bare reference is an account: the
        // transport reads them as it would for one.
        assert!(!read(json!({
            "error": { "code": "Base.1.8.Success", "message": "Success" },
        })));
        assert!(!read(json!({ "@Message.ExtendedInfo": [] })));
        assert!(!read(json!({
            "@odata.id": "/redfish/v1/AccountService/Accounts/1",
        })));
    }
}
