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

//! NVIDIA Fabric OEM extension.

use crate::oem::nvidia::schema::nvidia_fabric::NvidiaFabric as NvidiaFabricSchema;
use crate::oem::nvidia::OEM_KEY;
use crate::oem::oem_object;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;
use nv_redfish_core::Bmc;
use std::marker::PhantomData;
use std::sync::Arc;

/// NVIDIA OEM extension of a Redfish `Fabric`.
pub struct NvidiaFabric<B: Bmc> {
    data: Arc<NvidiaFabricSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> NvidiaFabric<B> {
    /// Read the extension out of a `Fabric` OEM payload.
    ///
    /// Returns `Ok(None)` when the payload carries no NVIDIA object,
    /// including when it carries an explicit `null`.
    pub(crate) fn new(oem: &ResourceOemSchema) -> Result<Option<Self>, Error<B>> {
        Ok(oem_object(oem, OEM_KEY)?.map(|data| Self {
            data,
            _marker: PhantomData,
        }))
    }

    /// Get the raw schema data for this NVIDIA fabric extension.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<NvidiaFabricSchema> {
        self.data.clone()
    }

    /// URI used to push switch configuration to this fabric.
    #[must_use]
    pub fn switch_config_push_uri(&self) -> Option<&str> {
        self.data
            .switch_config_push_uri
            .as_ref()
            .and_then(Option::as_deref)
    }
}
