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

//! Redfish Session - high-level wrapper.

use crate::schema::session::Session as SessionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use std::sync::Arc;

/// Represents a Redfish `Session`.
pub struct Session<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<SessionSchema>,
    auth_token: Option<String>,
    delete_location: Option<ODataId>,
}

impl<B: Bmc> Session<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SessionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
                auth_token: None,
                delete_location: None,
            })
    }

    pub(crate) fn from_data_with_session_metadata(
        bmc: NvBmc<B>,
        data: SessionSchema,
        auth_token: Option<String>,
        delete_location: Option<ODataId>,
    ) -> Self {
        Self {
            bmc,
            data: Arc::new(data),
            auth_token,
            delete_location,
        }
    }

    /// Get the raw schema data for this session.
    #[must_use]
    pub fn raw(&self) -> Arc<SessionSchema> {
        self.data.clone()
    }

    /// Get the authentication token returned when this session was created.
    #[must_use]
    pub fn auth_token(&self) -> Option<&str> {
        self.auth_token.as_deref()
    }

    /// Get the session URI returned in the creation response `Location` header.
    #[must_use]
    pub const fn location(&self) -> Option<&ODataId> {
        self.delete_location.as_ref()
    }

    /// Delete the current session.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the session returned by the
    ///   server.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if deletion fails.
    pub async fn delete(&self) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .delete::<NavProperty<SessionSchema>>(
                self.delete_location
                    .as_ref()
                    .unwrap_or_else(|| self.data.odata_id()),
            )
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }
}
