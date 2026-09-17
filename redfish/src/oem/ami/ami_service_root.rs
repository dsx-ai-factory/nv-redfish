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

//! Support AMI Service root extensions

use crate::core::Bmc;
use crate::oem::ami::schema::ami_service_root::AmiServiceRoot as AmiServiceRootSchema;
use crate::oem::oem_object;
use crate::schema::service_root::ServiceRoot as ServiceRootSchema;
use crate::Error;
use crate::NvBmc;
use std::marker::PhantomData;
use std::sync::Arc;

/// Represents an AMI OEM extension to the ServiceRoot schema.
pub struct AmiServiceRoot<B: Bmc> {
    data: Arc<AmiServiceRootSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> AmiServiceRoot<B> {
    /// Create a new AMI service root BMC handle.
    ///
    /// Returns `Ok(None)` when the service root does not include `Oem.Ami`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing OEM object data fails.
    pub(crate) fn new(
        _bmc: &NvBmc<B>,
        service_root: &ServiceRootSchema,
    ) -> Result<Option<Self>, Error<B>> {
        Ok(service_root
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Ami"))?
            .map(|data| Self {
                data,
                _marker: PhantomData,
            }))
    }

    /// Get the raw schema data for this BMC config.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<AmiServiceRootSchema> {
        self.data.clone()
    }

    /// Get Redfish Technology Pack version from the OEM extension.
    pub fn rtp_version(&self) -> Option<&str> {
        self.data.rtp_version.as_ref().and_then(Option::as_deref)
    }
}
