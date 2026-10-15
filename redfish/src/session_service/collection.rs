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

//! Session collection utilities.

use crate::schema::session::Session as SessionSchema;
use crate::schema::session_collection::SessionCollection as SessionCollectionSchema;
use crate::session_service::Session;
use crate::session_service::SessionCreate;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::NavProperty;
use std::sync::Arc;

/// Session collection.
///
/// Provides functions to list and create sessions.
pub struct SessionCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<SessionCollectionSchema>,
}

impl<B: Bmc> SessionCollection<B> {
    pub(crate) async fn new(
        bmc: NvBmc<B>,
        collection_ref: &NavProperty<SessionCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = bmc.expand_property(collection_ref).await?;
        Ok(Self { bmc, collection })
    }

    /// List all sessions available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching session data fails.
    pub async fn members(&self) -> Result<Vec<Session<B>>, Error<B>> {
        let mut members = Vec::with_capacity(self.collection.members.len());
        for member in &self.collection.members {
            members.push(Session::new(&self.bmc, member).await?);
        }
        Ok(members)
    }

    /// Create a new session.
    ///
    /// # Errors
    ///
    /// Returns an error if creating the session fails.
    pub async fn create_session(&self, create: &SessionCreate) -> Result<Session<B>, Error<B>> {
        let response = self
            .bmc
            .as_ref()
            .create_session::<_, SessionSchema>(self.collection.as_ref().odata_id(), create)
            .await
            .map_err(Error::from)?;
        Ok(Session::from_data_with_session_metadata(
            self.bmc.clone(),
            response.entity,
            Some(response.auth_token),
            Some(response.location),
        ))
    }
}
