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

//! `OData` attributes captured from annotations, used by code generation.

use crate::odata::annotations::AdditionalProperties;
use crate::odata::annotations::Deletable;
use crate::odata::annotations::DescriptionRef;
use crate::odata::annotations::Insertable;
use crate::odata::annotations::LongDescriptionRef;
use crate::odata::annotations::ODataAnnotations;
use crate::odata::annotations::Permissions;
use crate::odata::annotations::Updatable;
use tagged_types::TaggedType;

/// Whether the type must include `@odata.id` in generated code.
pub type MustHaveId = TaggedType<bool, MustHaveIdTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum MustHaveIdTag {}

/// Whether the type must include `@odata.type` in generated code.
pub type MustHaveType = TaggedType<bool, MustHaveTypeTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Debug)]
#[capability(inner_access)]
pub enum MustHaveTypeTag {}

/// `OData` attributes attached to compiled entities.
#[derive(Debug, Clone, Copy)]
pub struct OData<'a> {
    /// Whether `@odata.id` must be present.
    pub must_have_id: MustHaveId,
    /// Whether `@odata.type` must be present.
    pub must_have_type: MustHaveType,
    /// Short description.
    pub description: Option<DescriptionRef<'a>>,
    /// Long description.
    pub long_description: Option<LongDescriptionRef<'a>>,
    /// Permissions for the element.
    pub permissions: Option<Permissions>,
    /// Whether the value is serialized as a URI string.
    pub is_url: bool,
    /// Additional properties can be added.
    pub additional_properties: Option<AdditionalProperties>,
    /// Insertability (Capabilities.InsertRestrictions).
    pub insertable: Option<Insertable<'a>>,
    /// Updatability (Capabilities.UpdateRestrictions).
    pub updatable: Option<Updatable<'a>>,
    /// Deletability (Capabilities.DeleteRestrictions).
    pub deletable: Option<Deletable<'a>>,
}

impl<'a> OData<'a> {
    /// Create a new instance from an object that provides `OData` annotations.
    pub fn new(must_have_id: MustHaveId, src: &'a impl ODataAnnotations) -> Self {
        Self {
            must_have_id,
            must_have_type: MustHaveType::new(false),
            description: src.odata_description(),
            long_description: src.odata_long_description(),
            permissions: src.odata_permissions(),
            is_url: src.odata_is_url(),
            additional_properties: src.odata_additional_properties(),
            insertable: src.capabilities_insertable(),
            updatable: src.capabilities_updatable(),
            deletable: src.capabilities_deletable(),
        }
    }

    /// Whether no OData-related attributes are present.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.description.is_none()
            && self.long_description.is_none()
            && self.permissions.is_none()
            && !self.is_url
            && self.insertable.is_none()
            && self.updatable.is_none()
            && self.deletable.is_none()
    }

    /// Property is explicitly `Write` only.
    #[must_use]
    pub fn permissions_is_write_only(&self) -> bool {
        self.permissions.is_some_and(|v| v == Permissions::Write)
    }

    /// Property is writable (not strictly `Read`).
    #[must_use]
    pub fn permissions_is_write(&self) -> bool {
        self.permissions.is_none_or(|v| v != Permissions::Read)
    }
}
