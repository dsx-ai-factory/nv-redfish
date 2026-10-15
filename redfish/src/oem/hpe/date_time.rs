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

//! HPE iLO date and time settings.

use std::sync::Arc;

use nv_redfish_core::{Bmc, EntityTypeRef as _, ModificationResponse, NavProperty};

use crate::oem::hpe::schema::hpei_lo_date_time::HpeiLoDateTime as HpeiLoDateTimeSchema;
use crate::{Error, NvBmc};

#[doc(inline)]
pub use crate::oem::hpe::schema::hpei_lo_date_time::{
    HpeiLoDateTimeUpdate, TimeZone, TimeZoneUpdate,
};

/// HPE iLO date and time settings resource.
pub struct HpeiLoDateTime<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<HpeiLoDateTimeSchema>,
}

impl<B: Bmc> HpeiLoDateTime<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<HpeiLoDateTimeSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Statically configured NTP servers.
    #[must_use]
    pub fn static_ntp_servers(&self) -> Option<&[String]> {
        self.data.static_ntp_servers.as_deref()
    }

    /// Whether iLO propagates its time to the host after AC power is applied.
    #[must_use]
    pub fn propagate_time_to_host(&self) -> Option<bool> {
        self.data.propagate_time_to_host
    }

    /// Current iLO time zone.
    #[must_use]
    pub fn time_zone(&self) -> Option<&TimeZone> {
        self.data.time_zone.as_ref()
    }

    /// Update this date and time resource.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned entity fails.
    pub async fn update(
        &self,
        update: &HpeiLoDateTimeUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<HpeiLoDateTimeSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }

    /// Get the raw HPE date and time schema.
    #[must_use]
    pub fn raw(&self) -> Arc<HpeiLoDateTimeSchema> {
        self.data.clone()
    }
}
