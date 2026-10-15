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

//! Update Service entities and collections.
//!
//! This module provides types for working with Redfish UpdateService resources
//! and their sub-resources like firmware and software inventory.

mod software_inventory;

use std::sync::Arc;
use std::time::Duration;

use crate::core::NavProperty;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaUpdateServiceActions;
use crate::schema::update_service::UpdateService as UpdateServiceSchema;
use crate::schema::update_service::UpdateServiceSimpleUpdateAction;
use crate::schema::ActionAnnotations;
use crate::Error;
use crate::NvBmc;
use crate::ServiceRoot;

use nv_redfish_core::Bmc;
use nv_redfish_core::DataStream;
use nv_redfish_core::EntityTypeRef as _;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::HttpPushUriUpdateRequest;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::MultipartUpdateRequest;
use nv_redfish_core::UploadReader;
#[cfg(feature = "update-service-deprecated")]
use nv_redfish_core::UploadStream;
use software_inventory::SoftwareInventoryCollection;

#[doc(inline)]
pub use crate::schema::settings::OperationApplyTime;
#[doc(inline)]
pub use crate::schema::update_service::TransferProtocolType;
#[doc(inline)]
pub use crate::schema::update_service::UpdateParametersUpdate as MultipartUpdateParameters;
#[doc(inline)]
pub use crate::schema::update_service::UpdateServiceUpdate;
#[doc(inline)]
pub use software_inventory::SoftwareInventory;
#[doc(inline)]
pub use software_inventory::Version;
#[doc(inline)]
pub use software_inventory::VersionRef;

/// Multipart update parameters with `@Redfish.OperationApplyTime`.
///
/// The Redfish specification allows the annotation in multipart update
/// parameters, but the schema type can't carry it.
#[derive(serde::Serialize, Debug, Default)]
pub struct MultipartUpdateParametersWithApplyTime {
    /// Update parameters.
    #[serde(flatten)]
    pub parameters: MultipartUpdateParameters,
    /// When the service applies the update.
    #[serde(
        rename = "@Redfish.OperationApplyTime",
        skip_serializing_if = "Option::is_none"
    )]
    pub operation_apply_time: Option<OperationApplyTime>,
}

/// Update service.
///
/// Provides functions to access firmware and software inventory, and perform update actions.
pub struct UpdateService<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<UpdateServiceSchema>,
}

impl<B: Bmc> UpdateService<B> {
    /// Create a new update service handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        root: &ServiceRoot<B>,
    ) -> Result<Option<Self>, Error<B>> {
        if let Some(nav) = &root.root.update_service {
            nav.get(bmc.as_ref()).await.map_err(Error::from).map(Some)
        } else if bmc.quirks.bug_missing_root_nav_properties() {
            NavProperty::new_reference(format!("{}/UpdateService", root.root.odata_id).into())
                .get(bmc.as_ref())
                .await
                .map_err(Error::from)
                .map(Some)
        } else {
            Ok(None)
        }
        .map(|d| {
            d.map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
        })
    }

    /// Get the raw schema data for this update service.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<UpdateServiceSchema> {
        self.data.clone()
    }

    /// Get the NVIDIA OEM actions advertised by this update service.
    ///
    /// Returns `Ok(None)` when the service has no OEM actions object.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM actions cannot be parsed.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia_actions(&self) -> Result<Option<NvidiaUpdateServiceActions<B>>, Error<B>> {
        self.data
            .actions
            .as_ref()
            .and_then(|actions| actions.oem.as_ref())
            .map(|actions| NvidiaUpdateServiceActions::new(&self.bmc, actions))
            .transpose()
    }

    /// List all firmware inventory items.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The update service does not have a firmware inventory collection
    /// - Fetching firmware inventory data fails
    pub async fn firmware_inventories(
        &self,
    ) -> Result<Option<Vec<SoftwareInventory<B>>>, Error<B>> {
        if let Some(collection_ref) = &self.data.firmware_inventory {
            SoftwareInventoryCollection::new(&self.bmc, collection_ref)
                .await?
                .members()
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// List all software inventory items.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The update service does not have a software inventory collection
    /// - Fetching software inventory data fails
    pub async fn software_inventories(
        &self,
    ) -> Result<Option<Vec<SoftwareInventory<B>>>, Error<B>> {
        if let Some(collection_ref) = &self.data.software_inventory {
            let collection = self.bmc.expand_property(collection_ref).await?;
            let mut items = Vec::new();
            for item_ref in &collection.members {
                items.push(SoftwareInventory::new(&self.bmc, item_ref).await?);
            }
            Ok(Some(items))
        } else {
            Ok(None)
        }
    }

    /// Perform a simple update with the specified image URI.
    ///
    /// This action updates software components by downloading and installing
    /// a software image from the specified URI.
    ///
    /// # Arguments
    ///
    /// * `image_uri` - The URI of the software image to install
    /// * `transfer_protocol` - Optional network protocol to use for retrieving the image
    /// * `targets` - Optional list of URIs indicating where to apply the update
    /// * `username` - Optional username for accessing the image URI
    /// * `password` - Optional password for accessing the image URI
    /// * `force_update` - Whether to bypass update policies (e.g., allow downgrade)
    /// * `stage` - Whether to stage the image for later activation instead of immediate
    ///   installation
    /// * `local_image` - An indication of whether the service adds the image to the local image
    ///   store
    /// * `exclude_targets` - An array of URIs that indicate where not to apply the update image
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The update service does not support the `SimpleUpdate` action
    /// - The action execution fails
    #[allow(clippy::too_many_arguments)]
    pub async fn simple_update(
        &self,
        image_uri: String,
        transfer_protocol: Option<TransferProtocolType>,
        targets: Option<Vec<String>>,
        username: Option<String>,
        password: Option<String>,
        force_update: Option<bool>,
        stage: Option<bool>,
        local_image: Option<bool>,
        exclude_targets: Option<Vec<String>>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;

        actions
            .simple_update(
                self.bmc.as_ref(),
                &UpdateServiceSimpleUpdateAction {
                    redfish_annotations: ActionAnnotations::default(),
                    image_uri,
                    transfer_protocol,
                    targets,
                    username,
                    password,
                    force_update,
                    stage,
                    local_image,
                    exclude_targets,
                },
            )
            .await
            .map_err(Error::from)
    }

    /// Start updates that have been previously invoked with an `OperationApplyTime` of
    /// `OnStartUpdateRequest`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The update service does not support the `StartUpdate` action
    /// - The action execution fails
    pub async fn start_update(&self) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;

        actions
            .start_update(self.bmc.as_ref())
            .await
            .map_err(Error::from)
    }

    /// Update this service with generated `UpdateServiceUpdate` fields.
    ///
    /// This also supports composing OEM fields into the standard update.
    ///
    /// # Errors
    ///
    /// Returns an error if the update request fails.
    pub async fn update(
        &self,
        update: &UpdateServiceUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .update::<_, NavProperty<UpdateServiceSchema>>(
                self.data.odata_id(),
                self.data.etag(),
                update,
            )
            .await?
            .try_map_entity_async(|nav| async move {
                let data = nav.get(self.bmc.as_ref()).await.map_err(Error::from)?;

                Ok(Self {
                    bmc: self.bmc.clone(),
                    data,
                })
            })
            .await
    }

    /// Upload a raw binary stream using this service's deprecated `HttpPushUri`.
    ///
    /// The stream is sent as `application/octet-stream` without multipart
    /// `UpdateParameters`.
    ///
    /// # Errors
    ///
    /// Returns an error if `HttpPushUri` is absent or the upload fails.
    #[cfg(feature = "update-service-deprecated")]
    pub async fn http_push_uri_update_from_reader<U, R>(
        &self,
        update_stream: UploadStream<U>,
        upload_timeout: Duration,
    ) -> Result<ModificationResponse<R>, Error<B>>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        self.http_push_uri_update(HttpPushUriUpdateRequest {
            update_stream,
            upload_timeout,
        })
        .await
    }

    /// Perform a raw binary upload using this service's deprecated `HttpPushUri`.
    ///
    /// # Errors
    ///
    /// Returns an error if `HttpPushUri` is absent or the upload fails.
    #[cfg(feature = "update-service-deprecated")]
    pub async fn http_push_uri_update<U, R>(
        &self,
        request: HttpPushUriUpdateRequest<U>,
    ) -> Result<ModificationResponse<R>, Error<B>>
    where
        U: UploadReader,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        let http_push_uri = self
            .data
            .http_push_uri
            .as_ref()
            .ok_or(Error::UpdateServiceHttpPushUriNotAvailable)?;

        self.bmc
            .as_ref()
            .http_push_uri_update(http_push_uri, request)
            .await
            .map_err(Error::from)
    }

    /// Upload a named stream using this service's `MultipartHttpPushUri`.
    ///
    /// Prefer the generated [`MultipartUpdateParameters`] type. A generic
    /// payload is accepted for platform fields that are not generated.
    ///
    /// # Errors
    ///
    /// Returns an error if `MultipartHttpPushUri` is absent or the upload fails.
    pub async fn multipart_update_from_reader<U, V, R>(
        &self,
        update_parameters: &V,
        update_stream: DataStream<U>,
        upload_timeout: Duration,
    ) -> Result<ModificationResponse<R>, Error<B>>
    where
        U: UploadReader,
        V: Send + Sync + serde::Serialize,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        self.multipart_update(MultipartUpdateRequest {
            update_parameters,
            update_stream,
            oem_parts: Vec::new(),
            upload_timeout,
        })
        .await
    }

    /// Perform a multipart upload using this service's `MultipartHttpPushUri`.
    ///
    /// Use this method when the request needs optional OEM multipart parts.
    ///
    /// # Errors
    ///
    /// Returns an error if `MultipartHttpPushUri` is absent or the upload fails.
    pub async fn multipart_update<U, V, R>(
        &self,
        request: MultipartUpdateRequest<'_, U, V>,
    ) -> Result<ModificationResponse<R>, Error<B>>
    where
        U: UploadReader,
        V: Send + Sync + serde::Serialize,
        R: Send + Sync + for<'de> serde::Deserialize<'de>,
    {
        let multipart_uri = self
            .data
            .multipart_http_push_uri
            .as_ref()
            .ok_or(Error::UpdateServiceMultipartHttpPushUriNotAvailable)?;

        self.bmc
            .as_ref()
            .multipart_update(multipart_uri, request)
            .await
            .map_err(Error::from)
    }
}
