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

//! Support HPE Manager OEM extension.

use crate::oem::hpe::schema::hpei_lo_service_ext::HpeiLoServiceExt as HpeiLoServiceExtSchema;
use crate::oem::oem_object;
use crate::schema::service_root::ServiceRoot as ServiceRootSchema;
use crate::Error;
use nv_redfish_core::Bmc;
use std::marker::PhantomData;
use std::sync::Arc;

/// Represents an HPE OEM extension to Manager schema.
pub struct HpeiLoServiceExt<B: Bmc> {
    data: Arc<HpeiLoServiceExtSchema>,
    _bmc: PhantomData<B>,
}

impl<B: Bmc> HpeiLoServiceExt<B> {
    /// Create a new HPE iLo service extension wrapper.
    ///
    /// Returns `Ok(None)` when the manager does not include `Oem.Hpe`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing HPE extension OEM data fails.
    pub(crate) fn new(manager: &ServiceRootSchema) -> Result<Option<Self>, Error<B>> {
        Ok(manager
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, "Hpe"))?
            .map(|data| Self {
                data,
                _bmc: PhantomData,
            }))
    }

    /// Get the raw schema data for this HPE Manager.
    #[must_use]
    pub fn raw(&self) -> Arc<HpeiLoServiceExtSchema> {
        self.data.clone()
    }

    /// Manager type.
    #[must_use]
    pub fn manager_type(&self) -> Option<ManagerType<'_>> {
        self.data.manager.iter().flatten().find_map(|v| {
            v.manager_type.as_ref()?.as_deref().map(|v| {
                if let Some(("iLO", n)) = v.split_once(' ') {
                    n.parse::<u16>()
                        .map_or(ManagerType::Other(v), ManagerType::Ilo)
                } else {
                    ManagerType::Other(v)
                }
            })
        })
    }
}

/// Version of the Manager identified by HPE OEM extension in service
/// root.
#[derive(Clone, Copy)]
pub enum ManagerType<'a> {
    /// iLO x where x is Number
    Ilo(u16),
    /// Unknown version of iLO.
    Other(&'a str),
}
