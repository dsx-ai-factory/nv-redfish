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

//! Generic lightweight entity link.
//!
//! [`EntityLink<B, T>`](crate::entity_link::EntityLink) is an owned handle that pairs a BMC client with a
//! navigation property. It provides lazy access to any Redfish entity
//! without eagerly fetching it.
//!
//! Capabilities are determined by trait bounds on the schema type `T`:
//! - [`fetch`](crate::entity_link::EntityLink::fetch) — always available (requires `T: EntityTypeRef + Deserialize + Send + Sync`)
//! - [`delete`](crate::entity_link::EntityLink::delete) — available when `T: Deletable`
//!
//! Concrete link types are defined as type aliases:
//! ```ignore
//! pub type SensorLink<B> = EntityLink<B, SchemaSensor>;
//! pub type MetricReportLink<B> = EntityLink<B, MetricReportSchema>;
//! ```

use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::Deletable;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use serde::Deserialize;
use std::future::Future;
use std::sync::Arc;

/// Lightweight owned handle to a Redfish entity.
///
/// Holds a cloned BMC client and a [`NavProperty<T>`] without eagerly
/// fetching the entity. Use [`fetch`](Self::fetch) to retrieve the
/// data on demand.
///
/// Operations like [`delete`](Self::delete) are available when the
/// schema type `T` implements the corresponding core trait.
pub struct EntityLink<B: Bmc, T: EntityTypeRef> {
    bmc: NvBmc<B>,
    nav: NavProperty<T>,
}

impl<B: Bmc, T: EntityTypeRef> EntityLink<B, T> {
    /// Create a new entity link.
    pub(crate) fn new(bmc: &NvBmc<B>, nav: NavProperty<T>) -> Self {
        Self {
            bmc: bmc.clone(),
            nav,
        }
    }

    /// `OData` identifier of this entity.
    ///
    /// Always available without network I/O.
    #[must_use]
    pub fn odata_id(&self) -> &ODataId {
        self.nav.id()
    }

    /// Access the underlying navigation property.
    #[must_use]
    pub const fn nav(&self) -> &NavProperty<T> {
        &self.nav
    }
}

impl<B, T> EntityLink<B, T>
where
    B: Bmc,
    T: EntityTypeRef + for<'de> Deserialize<'de> + 'static,
{
    /// Fetch the entity from the BMC.
    ///
    /// If the navigation property is already expanded, returns the
    /// cached value without network I/O.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the entity fails.
    pub async fn fetch(&self) -> Result<Arc<T>, Error<B>> {
        self.nav.get(self.bmc.as_ref()).await.map_err(Error::from)
    }

    /// Construct a full wrapper from this link.
    ///
    /// Fetches the entity and wraps it in a higher-level type
    /// that provides richer API access.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the entity fails.
    pub async fn upgrade<W>(&self) -> Result<W, Error<B>>
    where
        W: FromLink<B, Schema = T>,
    {
        W::from_link(&self.bmc, &self.nav).await
    }
}

impl<B, T> EntityLink<B, T>
where
    B: Bmc,
    T: Deletable,
{
    /// Delete this entity.
    ///
    /// Only available when the schema type implements [`Deletable`].
    ///
    /// # Errors
    ///
    /// Returns an error if deleting the entity fails.
    pub async fn delete(&self) -> Result<ModificationResponse<T>, Error<B>> {
        self.bmc
            .as_ref()
            .delete(self.odata_id())
            .await
            .map_err(Error::from)
    }
}

/// Trait for full wrapper types that can be constructed from an entity link.
///
/// Implemented by wrapper types that need only `NvBmc<B>` and
/// `NavProperty<T>` to be constructed (i.e., no extra context).
pub trait FromLink<B: Bmc>: Sized {
    /// The schema type this wrapper is built from.
    type Schema: EntityTypeRef + for<'de> Deserialize<'de>;

    /// Construct the full wrapper by fetching entity data.
    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send;
}
