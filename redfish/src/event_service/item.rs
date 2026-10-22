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

//! Redfish `EventDestination` - high-level wrapper.

use std::sync::Arc;

use crate::schema::event_destination::EventDestination as EventDestinationSchema;
use crate::schema::event_destination::SubscriptionType;
use crate::Error;
use crate::NvBmc;

use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;

/// Represents a Redfish `EventDestination`.
pub struct EventDestination<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<EventDestinationSchema>,
}

impl<B: Bmc> EventDestination<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<EventDestinationSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this event destination.
    #[must_use]
    pub fn raw(&self) -> Arc<EventDestinationSchema> {
        self.data.clone()
    }

    /// Get the subscription type, or `None` when the service reports `null`.
    #[must_use]
    pub fn subscription_type(&self) -> Option<SubscriptionType> {
        self.data.subscription_type
    }

    /// Delete this event destination.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the event destination
    ///   returned by the server.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if deleting the entity fails.
    pub async fn delete(&self) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .delete::<NavProperty<EventDestinationSchema>>(self.data.odata_id())
            .await
            .map_err(Error::from)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }
}
