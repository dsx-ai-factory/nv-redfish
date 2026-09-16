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

use crate::core::Bmc;
use crate::core::EdmPrimitiveType;
use crate::oem::dell::schema::dell_attributes::DellAttributes as DellAttributesSchema;
use crate::oem::oem_value;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "managers")]
use crate::core::EntityTypeRef as _;
#[cfg(feature = "managers")]
use crate::core::NavProperty;
#[cfg(feature = "managers")]
use crate::core::ODataId;
#[cfg(feature = "managers")]
use crate::schema::manager::Manager as ManagerSchema;
#[cfg(feature = "managers")]
use crate::Error;
#[cfg(feature = "managers")]
use crate::NvBmc;

/// Dell OEM Attributes.
pub struct DellAttributes<B: Bmc> {
    data: Arc<DellAttributesSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> DellAttributes<B> {
    /// Create Dell OEM Manager attributes.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.Dell`.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching or parsing Dell attributes data fails.
    #[cfg(feature = "managers")]
    pub(crate) async fn manager_attributes(
        bmc: &NvBmc<B>,
        manager: &ManagerSchema,
    ) -> Result<Option<Self>, Error<B>> {
        if manager
            .oem
            .as_ref()
            .is_some_and(|oem| oem_value(oem, "Dell").is_some())
        {
            // Dell doesn't provide navigation property to the
            // Attributes from the Manager. So we just craft @odata.id
            // for it.
            let odata_id = ODataId::from(format!(
                "{}/Oem/Dell/DellAttributes/{}",
                manager.odata_id(),
                manager.id
            ));
            bmc.expand_property(&NavProperty::new_reference(odata_id))
                .await
                .map(|data| Self {
                    data,
                    _marker: PhantomData,
                })
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get attribute by key value.
    #[must_use]
    pub fn attribute<'a>(&'a self, name: &str) -> Option<DellAttributeRef<'a>> {
        self.data
            .attributes
            .as_ref()
            .and_then(|attributes| attributes.dynamic_properties.get(name))
            .map(|v| DellAttributeRef::new(v.as_ref()))
    }
}

/// Reference to a BIOS attribute.
pub struct DellAttributeRef<'a> {
    value: Option<&'a EdmPrimitiveType>,
}

impl<'a> DellAttributeRef<'a> {
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
