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

//! Control resources.
//!
//! # Example
//!
//! ```ignore
//! use nv_redfish::control::ControlUpdate;
//! use nv_redfish_core::ModificationResponse;
//!
//! let Some(power_limit) = chassis.environment_power_limit_control().await? else {
//!     return Ok(());
//! };
//!
//! let update = ControlUpdate::builder().with_set_point(700.0).build();
//!
//! match power_limit.update(&update).await? {
//!     ModificationResponse::Entity(updated) => {
//!         let _updated_control = updated.raw();
//!     }
//!     ModificationResponse::Task(task) => {
//!         let _task = task;
//!     }
//!     ModificationResponse::Empty => {}
//! }
//! ```

use std::sync::Arc;

use crate::schema::control::Control as ControlSchema;
#[cfg(feature = "chassis")]
use crate::schema::control_collection::ControlCollection as ControlCollectionSchema;
use crate::Error;
use crate::NvBmc;

use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;

pub use crate::schema::control::ControlMode;
pub use crate::schema::control::ControlType;
pub use crate::schema::control::ControlUpdate;
pub use crate::schema::control::ImplementationType;
pub use crate::schema::control::SetPointType;

/// Control collection.
///
/// This wraps the collection resource and its member links. Per-control
/// properties such as set point and allowable range are stored on each
/// [`Control`] returned by [`Self::members`].
///
/// # Example
///
/// ```ignore
/// let controls = control_collection.members().await?;
///
/// for control in controls {
///     let _control = control.raw();
/// }
/// ```
#[cfg(feature = "chassis")]
pub struct ControlCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<ControlCollectionSchema>,
}

#[cfg(feature = "chassis")]
impl<B: Bmc> ControlCollection<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ControlCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        // Read the collection from the BMC so members can be fetched later.
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|collection| Self {
                bmc: bmc.clone(),
                collection,
            })
    }

    /// Get the raw control collection schema data.
    #[must_use]
    pub fn raw(&self) -> Arc<ControlCollectionSchema> {
        self.collection.clone()
    }

    /// List all controls in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching a control fails.
    pub async fn members(&self) -> Result<Vec<Control<B>>, Error<B>> {
        let mut controls = Vec::with_capacity(self.collection.members.len());

        for control in &self.collection.members {
            controls.push(Control::new(&self.bmc, control).await?);
        }

        Ok(controls)
    }
}

/// Control entity wrapper.
///
/// The raw schema data contains the target BMC's reported control properties,
/// including set point, units, allowable range, and related metadata when the
/// service provides them.
pub struct Control<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<ControlSchema>,
}

impl<B: Bmc> Control<B> {
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ControlSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw control schema data.
    #[must_use]
    pub fn raw(&self) -> Arc<ControlSchema> {
        self.data.clone()
    }

    /// Update this control.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use nv_redfish::control::ControlUpdate;
    /// use nv_redfish::core::ModificationResponse;
    ///
    /// let update = ControlUpdate::builder().with_set_point(700.0).build();
    ///
    /// match power_limit.update(&update).await? {
    ///     ModificationResponse::Entity(updated) => {
    ///         let _updated_control = updated.raw();
    ///     }
    ///     ModificationResponse::Task(task) => {
    ///         let _update_task = task;
    ///     }
    ///     ModificationResponse::Empty => {}
    /// }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if updating the control fails.
    pub async fn update(
        &self,
        update: &ControlUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<ControlSchema>>(self.data.odata_id(), self.data.etag(), update)
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move { Self::new(&self.bmc, &nav).await })
            .await
    }
}
