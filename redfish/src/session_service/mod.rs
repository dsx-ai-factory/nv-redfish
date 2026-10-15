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

//! Session Service entities and helpers.
//!
//! This module provides typed access to Redfish `SessionService`, including
//! listing, creating, and deleting sessions.

mod collection;
mod item;

use crate::schema::session_service::SessionService as SessionServiceSchema;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use nv_redfish_core::Bmc;
use std::sync::Arc;

#[doc(inline)]
pub use crate::schema::session::SessionCreate;
#[doc(inline)]
pub use crate::schema::session::SessionTypes;
#[doc(inline)]
pub use collection::SessionCollection;
#[doc(inline)]
pub use item::Session;

/// Session service.
///
/// Provides access to the session collection and individual session resources.
pub struct SessionService<B: Bmc> {
    bmc: NvBmc<B>,
    service: Arc<SessionServiceSchema>,
}

impl<B: Bmc> SessionService<B> {
    /// Create a new session service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        if let Some(service_ref) = &root.root.session_service {
            let service = service_ref.get(bmc.as_ref()).await.map_err(Error::from)?;
            Ok(Some(Self {
                bmc: bmc.clone(),
                service,
            }))
        } else {
            Ok(None)
        }
    }

    /// Get the raw schema data for this session service.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<SessionServiceSchema> {
        self.service.clone()
    }

    /// Get the sessions collection.
    ///
    /// # Errors
    ///
    /// Returns an error if retrieving session collection data fails.
    pub async fn sessions(&self) -> Result<Option<SessionCollection<B>>, Error<B>> {
        if let Some(collection_ref) = self.service.sessions.as_ref() {
            SessionCollection::new(self.bmc.clone(), collection_ref)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }
}
