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

use crate::certificate::normalize_pem_chain_type;
use crate::certificate::Certificate;
use crate::core::action::ActionTarget;
use crate::core::ActionError;
use crate::core::Bmc;
use crate::core::EntityTypeRef;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::core::ODataETag;
use crate::core::ODataId;
use crate::patch_support::Payload;
use crate::patch_support::ReadPatchFn;
use crate::schema::component_integrity::ComponentIntegrity as ComponentIntegritySchema;
use crate::schema::component_integrity_collection::ComponentIntegrityCollection as ComponentIntegrityCollectionSchema;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;
use serde::Deserialize;

#[doc(inline)]
pub use crate::schema::component_integrity::ComponentIntegrityType;
#[doc(inline)]
pub use crate::schema::component_integrity::SpdmGetSignedMeasurementsResponse;

fn unknown_odata_id() -> ODataId {
    ODataId::from(String::new())
}

/// Fetch envelope for a compatibility endpoint whose CSDL return type is a
/// complex type rather than an entity and therefore has no `EntityTypeRef`.
#[derive(Deserialize)]
struct ResponseBody<T> {
    #[serde(rename = "@odata.id", default = "unknown_odata_id")]
    odata_id: ODataId,
    #[serde(rename = "@odata.etag", default)]
    odata_etag: Option<ODataETag>,
    #[serde(flatten)]
    data: T,
}

impl<T: Send + Sync> EntityTypeRef for ResponseBody<T> {
    fn odata_id(&self) -> &ODataId {
        &self.odata_id
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.odata_etag.as_ref()
    }
}

/// SPDM signed measurements retrieved from a compatibility data endpoint.
pub struct SpdmSignedMeasurements {
    data: Arc<ResponseBody<SpdmGetSignedMeasurementsResponse>>,
}

impl SpdmSignedMeasurements {
    /// Get the generated standard signed-measurements response.
    #[must_use]
    pub fn raw(&self) -> &SpdmGetSignedMeasurementsResponse {
        &self.data.data
    }
}

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
        let data = collection_ref.get(bmc.as_ref()).await.map_err(Error::Bmc)?;
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
        let data = component_ref.get(bmc.as_ref()).await.map_err(Error::Bmc)?;
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

        let data = if self.bmc.quirks.certificate_type_wrong_pem_chain_case() {
            let patch_fn = Arc::new(normalize_pem_chain_type) as ReadPatchFn;
            Payload::get(self.bmc.as_ref(), certificate_ref, patch_fn.as_ref()).await?
        } else {
            certificate_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?
        };
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
    /// The response preserves synchronous evidence, an asynchronous task, or
    /// an empty successful response.
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
    ) -> Result<ModificationResponse<SpdmGetSignedMeasurementsResponse>, Error<B>>
    where
        B::Error: ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;
        if actions.spdm_get_signed_measurements.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        actions
            .spdm_get_signed_measurements(self.bmc.as_ref(), nonce, slot_id, measurement_indices)
            .await
            .map_err(Error::Bmc)
    }

    /// Get the target advertised for the SPDM signed-measurements action.
    ///
    /// This is useful for services that expose the completed asynchronous
    /// response at a URI derived from the advertised action target.
    #[must_use]
    pub fn spdm_get_signed_measurements_target(&self) -> Option<&ActionTarget> {
        self.data
            .actions
            .as_ref()?
            .spdm_get_signed_measurements
            .as_ref()
            .map(|action| &action.target)
    }

    /// Fetch completed signed measurements from the action's data endpoint.
    ///
    /// Some NVIDIA services complete the asynchronous action by exposing the
    /// standard `SPDMGetSignedMeasurementsResponse` at:
    ///
    /// ```text
    /// GET <advertised ComponentIntegrity.SPDMGetSignedMeasurements target>/data
    /// ```
    ///
    /// The action target itself is always taken from the resource.
    ///
    /// # Errors
    ///
    /// Returns an error if the action is unavailable or fetching the data
    /// endpoint fails.
    pub async fn spdm_signed_measurements_data(&self) -> Result<SpdmSignedMeasurements, Error<B>> {
        let target = self
            .spdm_get_signed_measurements_target()
            .ok_or(Error::ActionNotAvailable)?;
        let data_id = ODataId::from(format!("{}/data", target.as_str().trim_end_matches('/')));
        self.bmc
            .as_ref()
            .get::<ResponseBody<SpdmGetSignedMeasurementsResponse>>(&data_id)
            .await
            .map(|data| SpdmSignedMeasurements { data })
            .map_err(Error::Bmc)
    }

    /// Get the raw schema data for this component-integrity resource.
    #[must_use]
    pub fn raw(&self) -> Arc<ComponentIntegritySchema> {
        self.data.clone()
    }
}
