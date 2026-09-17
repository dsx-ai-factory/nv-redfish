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
//! Bios

use crate::schema::bios::Bios as BiosSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EdmPrimitiveType;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::RedfishSettings as _;
use std::sync::Arc;

pub use crate::schema::bios::AttributesUpdate;
pub use crate::schema::bios::BiosUpdate;

/// BIOS.
///
/// Provides functions to access BIOS functions.
pub struct Bios<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<BiosSchema>,
}

impl<B: Bmc> Bios<B> {
    /// Create a new log bios handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<BiosSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for the BIOS.
    #[must_use]
    pub fn raw(&self) -> Arc<BiosSchema> {
        self.data.clone()
    }

    /// Get the advertised BIOS settings object.
    ///
    /// Returns `Ok(None)` when this BIOS resource does not advertise
    /// `@Redfish.Settings`.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the settings object fails.
    pub async fn settings(&self) -> Result<Option<Self>, Error<B>> {
        match self.data.settings_object() {
            Some(settings) => Self::new(&self.bmc, &settings).await.map(Some),
            None => Ok(None),
        }
    }

    /// Update this BIOS resource.
    ///
    /// Call this method on the handle returned by [`Self::settings`] when the
    /// service advertises `@Redfish.Settings`.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the BIOS resource fails.
    pub async fn update(
        &self,
        update: &BiosUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<BiosSchema>>(self.data.odata_id(), self.data.etag(), update)
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Get bios attribute by key value.
    #[must_use]
    pub fn attribute<'a>(&'a self, name: &str) -> Option<BiosAttributeRef<'a>> {
        self.data
            .attributes
            .as_ref()
            .and_then(|attributes| attributes.dynamic_properties.get(name))
            .map(|v| BiosAttributeRef::new(v.as_ref()))
    }
}

/// Reference to a BIOS attribute.
pub struct BiosAttributeRef<'a> {
    value: Option<&'a EdmPrimitiveType>,
}

impl<'a> BiosAttributeRef<'a> {
    const fn new(value: Option<&'a EdmPrimitiveType>) -> Self {
        Self { value }
    }

    /// Returns true if attribute is null.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        self.value.is_none()
    }

    /// Returns string value of the attribute if attribute is string.
    #[must_use]
    pub const fn str_value(&self) -> Option<&str> {
        match self.value {
            Some(EdmPrimitiveType::String(v)) => Some(v.as_str()),
            _ => None,
        }
    }

    /// Returns boolean value of the attribute if attribute is bool.
    #[must_use]
    pub const fn bool_value(&self) -> Option<bool> {
        match self.value {
            Some(EdmPrimitiveType::Bool(v)) => Some(*v),
            _ => None,
        }
    }

    /// Returns integer value of the attribute if attribute is integer.
    #[must_use]
    pub const fn integer_value(&self) -> Option<i64> {
        match self.value {
            Some(EdmPrimitiveType::Integer(v)) => Some(*v),
            _ => None,
        }
    }

    /// Returns decimal value of the attribute if attribute is decimal.
    #[must_use]
    pub const fn decimal_value(&self) -> Option<f64> {
        match self.value {
            Some(EdmPrimitiveType::Decimal(v)) => Some(*v),
            _ => None,
        }
    }
}
