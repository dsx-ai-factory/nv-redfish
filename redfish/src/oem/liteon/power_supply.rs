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

use crate::entity_link::EntityLink;
use crate::oem::liteon::schema::liteon_power_supply::LiteonPowerSupply as LiteonPowerSupplySchema;
use crate::oem::liteon::schema::liteon_power_supply_collection::LiteonPowerSupplyCollection as LiteonPowerSupplyCollectionSchema;

use crate::chassis::Chassis;
use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::NavProperty;
use crate::Error;
use crate::NvBmc;

/// Link for LiteOn power supply.
pub type LiteonPowerSupplyLink<B> = EntityLink<B, LiteonPowerSupplySchema>;

pub(crate) async fn chassis_fetch_links<B: Bmc>(
    bmc: &NvBmc<B>,
    chassis: &Chassis<B>,
) -> Result<Option<Vec<LiteonPowerSupplyLink<B>>>, Error<B>> {
    use crate::oem::liteon::CHASSIS_MANUFACTURER;
    if chassis.hardware_id().manufacturer != Some(CHASSIS_MANUFACTURER) {
        return Ok(None);
    }
    let Some(power_subsystem) = &chassis.raw().power_subsystem else {
        return Ok(None);
    };
    let power_subsystem = power_subsystem
        .get(bmc.as_ref())
        .await
        .map_err(Error::from)?;
    let Some(power_supplies) = &power_subsystem.power_supplies else {
        return Ok(None);
    };
    // Convert link to OEM format:
    NavProperty::<LiteonPowerSupplyCollectionSchema>::new_reference(
        power_supplies.odata_id().clone(),
    )
    .get(bmc.as_ref())
    .await
    .map_err(Error::from)
    .map(|v| {
        v.members
            .iter()
            .map(|v| LiteonPowerSupplyLink::new(bmc, NavProperty::new_reference(v.id().clone())))
            .collect()
    })
    .map(Some)
}
