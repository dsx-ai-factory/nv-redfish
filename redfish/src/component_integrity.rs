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

//! Standard Redfish component-integrity resources and SPDM operations.

use std::sync::Arc;

use crate::certificate::Certificate;
use crate::core::action::ActionTarget;
use crate::core::Bmc;
use crate::core::NavProperty;
use crate::schema::component_integrity::ComponentIntegrity as ComponentIntegritySchema;
use crate::schema::component_integrity::ComponentIntegritySPDMGetSignedMeasurementsAction;
use crate::schema::component_integrity_collection::ComponentIntegrityCollection as ComponentIntegrityCollectionSchema;
use crate::schema::ActionAnnotations;
use crate::task_service::AsyncActionResult;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;

#[doc(inline)]
pub use crate::schema::component_integrity::ComponentIntegrityType;
#[doc(inline)]
pub use crate::schema::component_integrity::SpdmGetSignedMeasurementsResponse;

/// Collection of standard Redfish component-integrity resources.
pub struct ComponentIntegrityCollection<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<ComponentIntegrityCollectionSchema>,
}

impl<B: Bmc> ComponentIntegrityCollection<B> {
    /// Create a collection from the link advertised by the service root.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        let Some(collection_ref) = &root.root.component_integrity else {
            return Ok(None);
        };
        let data = collection_ref
            .get(bmc.as_ref())
            .await
            .map_err(Error::from)?;
        Ok(Some(Self {
            bmc: bmc.clone(),
            data,
        }))
    }

    /// List the component-integrity resources advertised by this service.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching a collection member fails.
    pub async fn members(&self) -> Result<Vec<ComponentIntegrity<B>>, Error<B>> {
        let mut members = Vec::with_capacity(self.data.members.len());
        for member in &self.data.members {
            members.push(ComponentIntegrity::new(&self.bmc, member).await?);
        }
        Ok(members)
    }

    /// Get the raw schema data for this collection.
    #[must_use]
    pub fn raw(&self) -> Arc<ComponentIntegrityCollectionSchema> {
        self.data.clone()
    }
}

/// Standard Redfish integrity information for one component.
pub struct ComponentIntegrity<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<ComponentIntegritySchema>,
}

impl<B: Bmc> ComponentIntegrity<B> {
    /// Fetch a component-integrity resource from its collection member link.
    async fn new(
        bmc: &NvBmc<B>,
        component_ref: &NavProperty<ComponentIntegritySchema>,
    ) -> Result<Self, Error<B>> {
        let data = component_ref.get(bmc.as_ref()).await.map_err(Error::from)?;
        Ok(Self {
            bmc: bmc.clone(),
            data,
        })
    }

    /// Fetch the certificate that identifies the SPDM responder.
    ///
    /// Returns `Ok(None)` when the resource does not advertise an SPDM
    /// responder certificate.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the advertised certificate fails.
    pub async fn component_certificate(&self) -> Result<Option<Certificate>, Error<B>> {
        let Some(spdm) = &self.data.spdm else {
            return Ok(None);
        };
        let Some(identity) = spdm
            .identity_authentication
            .as_ref()
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let Some(responder) = identity
            .responder_authentication
            .as_ref()
            .and_then(Option::as_ref)
        else {
            return Ok(None);
        };
        let Some(certificate_ref) = &responder.component_certificate else {
            return Ok(None);
        };

        let data = certificate_ref
            .get(self.bmc.as_ref())
            .await
            .map_err(Error::from)?;
        Ok(Some(Certificate::new(data)))
    }

    /// Invoke the advertised SPDM signed-measurements action.
    ///
    /// This is equivalent to:
    ///
    /// ```text
    /// POST <advertised ComponentIntegrity.SPDMGetSignedMeasurements target>
    /// {
    ///   "Nonce": "<optional hex nonce>",
    ///   "SlotId": <optional certificate slot>,
    ///   "MeasurementIndices": [<optional measurement indices>]
    /// }
    /// ```
    ///
    /// Poll the returned handle with [`AsyncActionResult::poll_result`] until
    /// it completes. It returns the measurements whether the service answers
    /// synchronously, through a Task Monitor, or through a Task resource, and
    /// reads them from the result `Location` the service names when it does
    /// not return them directly.
    ///
    /// Result URIs are shared between requests, so verify the measurements'
    /// signature against the request nonce before trusting them.
    ///
    /// # Errors
    ///
    /// Returns an error if the resource does not advertise the action or if
    /// invoking the action fails.
    pub async fn spdm_get_signed_measurements(
        &self,
        nonce: Option<String>,
        slot_id: Option<i64>,
        measurement_indices: Option<Vec<i64>>,
    ) -> Result<AsyncActionResult<SpdmGetSignedMeasurementsResponse>, Error<B>> {
        let action = self
            .data
            .actions
            .as_ref()
            .and_then(|actions| actions.spdm_get_signed_measurements.as_ref())
            .ok_or(Error::ActionNotAvailable)?;
        let params = ComponentIntegritySPDMGetSignedMeasurementsAction {
            redfish_annotations: ActionAnnotations::default(),
            nonce,
            slot_id,
            measurement_indices,
        };
        // An action's reply is the transport's, so it starts on the
        // transport underneath the compatibility layer.
        AsyncActionResult::start(self.bmc.as_ref().inner().as_ref(), action, &params).await
    }

    /// Get the target advertised for the SPDM signed-measurements action.
    #[must_use]
    pub fn spdm_get_signed_measurements_target(&self) -> Option<&ActionTarget> {
        self.data
            .actions
            .as_ref()?
            .spdm_get_signed_measurements
            .as_ref()
            .map(|action| &action.target)
    }

    /// Get the raw schema data for this component-integrity resource.
    #[must_use]
    pub fn raw(&self) -> Arc<ComponentIntegritySchema> {
        self.data.clone()
    }
}
