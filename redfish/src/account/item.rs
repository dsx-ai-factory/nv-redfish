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
//! - `Config::read_patch_fn`: apply read-time JSON patches for vendor
//!   compatibility
//! - `Config::deletion_strategy`: delete a normal resource or safely disable a
//!   preallocated slot
//!
//! Note: `Account` objects are created by higher-level APIs (e.g.
//! `AccountCollection`) and do not create accounts on the BMC by themselves.
//! Use the collection to create new accounts.

use crate::account::ManagerAccountUpdate;
use crate::patch_support::Payload;
use crate::patch_support::ReadPatchFn;
use crate::patch_support::UpdateWithPatch;
use crate::schema::manager_account::ManagerAccount;
use crate::Error;
use crate::NvBmc;
use crate::Resource;
use crate::ResourceSchema;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::sync::Arc;

#[derive(Clone, Copy)]
pub enum DeletionStrategy {
    DeleteResource,
    DisableSlot,
}

#[derive(Clone)]
pub struct Config {
    /// Function to patch input JSON when reading account structures.
    pub read_patch_fn: Option<ReadPatchFn>,
    /// How account deletion is performed.
    pub deletion_strategy: DeletionStrategy,
}

/// Represents a Redfish `ManagerAccount`.
pub struct Account<B: Bmc> {
    config: Config,
    bmc: NvBmc<B>,
    data: Arc<ManagerAccount>,
}

impl<B: Bmc> UpdateWithPatch<ManagerAccount, ManagerAccountUpdate, B> for Account<B> {
    fn entity_ref(&self) -> &ManagerAccount {
        self.data.as_ref()
    }
    fn patch(&self) -> Option<&ReadPatchFn> {
        self.config.read_patch_fn.as_ref()
    }
    fn bmc(&self) -> &B {
        self.bmc.as_ref()
    }
}

impl<B: Bmc> Account<B> {
    /// Create a new account handle. This does not create an account on the
    /// BMC.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ManagerAccount>,
        config: &Config,
    ) -> Result<Self, Error<B>> {
        if let Some(read_patch_fn) = &config.read_patch_fn {
            Payload::get(bmc.as_ref(), nav, read_patch_fn.as_ref()).await
        } else {
            nav.get(bmc.as_ref()).await.map_err(Error::Bmc)
        }
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
        Ok(self
            .update_with_patch(update)
            .await?
            .map_entity(|ma| Self::from_data(self.bmc.clone(), ma, self.config.clone())))
    }

    /// Update the account's password.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated account.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
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
                    .map_err(Error::Bmc)?
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

impl<B: Bmc> Resource for Account<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.data.as_ref().base
    }
}
