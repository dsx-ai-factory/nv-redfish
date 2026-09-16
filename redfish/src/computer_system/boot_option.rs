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
//! Boot options
//!

use crate::computer_system::BootOptionReference;
use crate::patch_support::CollectionWithPatch;
use crate::schema::boot_option::BootOption as BootOptionSchema;
use crate::schema::boot_option_collection::BootOptionCollection as BootOptionCollectionSchema;
use crate::schema::resource::ResourceCollection;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::marker::PhantomData;
use std::sync::Arc;
use tagged_types::TaggedType;

/// Boot options collection.
///
/// Provides functions to access collection members.
pub struct BootOptionCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<BootOptionCollectionSchema>,
}

impl<B: Bmc> CollectionWithPatch<BootOptionCollectionSchema, BootOptionSchema, B>
    for BootOptionCollection<B>
{
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<BootOptionSchema>>,
    ) -> BootOptionCollectionSchema {
        BootOptionCollectionSchema {
            odata_id: base.odata_id,
            odata_etag: base.odata_etag,
            odata_type: base.odata_type,
            settings_annotations: base.settings_annotations,
            description: base.description,
            name: base.name,
            oem: base.oem,
            members,
        }
    }
}

impl<B: Bmc> BootOptionCollection<B> {
    /// Create a new manager collection handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<BootOptionCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = Self::expand_collection(bmc, nav, None, None).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List all managers available in this BMC.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching manager data fails.
    pub async fn members(&self) -> Result<Vec<BootOption<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(BootOption::new(&self.bmc, m).await?);
        }
        Ok(members)
    }
}

/// The UEFI device path to access this UEFI boot option.
///
/// Nv-redfish keeps open underlying type for `UefiDevicePath` because it
/// can really be represented by any implementation of UEFI's device path.
pub type UefiDevicePath<T> = TaggedType<T, UefiDevicePathTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, FromStr, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum UefiDevicePathTag {}

/// The user-readable display name of the boot option that appears in
/// the boot order list in the user interface.
pub type DisplayName<T> = TaggedType<T, DisplayNameTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum DisplayNameTag {}

/// Boot option.
///
/// Provides functions to access boot option.
pub struct BootOption<B: Bmc> {
    data: Arc<BootOptionSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> BootOption<B> {
    /// Create a new log service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<BootOptionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(crate::Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this boot option.
    #[must_use]
    pub fn raw(&self) -> Arc<BootOptionSchema> {
        self.data.clone()
    }

    ///
    /// Boot option reference.
    #[must_use]
    pub fn boot_reference(&self) -> BootOptionReference<&str> {
        self.data.boot_option_reference.as_deref().map_or_else(
            || BootOptionReference::new(&self.data.id),
            BootOptionReference::new,
        )
    }

    /// An indication of whether the boot option is enabled.
    #[must_use]
    pub fn enabled(&self) -> Option<bool> {
        self.data.boot_option_enabled.and_then(identity)
    }

    /// The user-readable display name of the boot option that appears
    /// in the boot order list in the user interface.
    #[must_use]
    pub fn display_name(&self) -> Option<DisplayName<&str>> {
        self.data
            .display_name
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(DisplayName::new)
    }

    /// The UEFI device path to access this UEFI boot option.
    #[must_use]
    pub fn uefi_device_path(&self) -> Option<UefiDevicePath<&str>> {
        self.data
            .uefi_device_path
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(UefiDevicePath::new)
    }
}
