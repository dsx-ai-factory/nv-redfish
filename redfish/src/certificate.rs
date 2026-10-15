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

//! Standard Redfish certificate resources.

use std::sync::Arc;

use crate::schema::certificate::Certificate as CertificateSchema;
use crate::schema::certificate_collection::CertificateCollection as CertificateCollectionSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef as _;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;

#[doc(inline)]
pub use crate::schema::certificate::CertificateCreate;
#[doc(inline)]
pub use crate::schema::certificate::CertificateType;
#[doc(inline)]
pub use crate::schema::certificate::CertificateUsageType;

/// A standard Redfish certificate.
pub struct Certificate {
    data: Arc<CertificateSchema>,
}

impl Certificate {
    /// Create a certificate from generated schema data.
    pub(crate) const fn new(data: Arc<CertificateSchema>) -> Self {
        Self { data }
    }

    /// Get the raw schema data for this certificate.
    #[must_use]
    pub fn raw(&self) -> Arc<CertificateSchema> {
        self.data.clone()
    }

    async fn from_nav<B: Bmc>(
        bmc: &NvBmc<B>,
        nav: &NavProperty<CertificateSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map(Self::new)
            .map_err(Error::from)
    }
}

/// Collection of standard Redfish certificates.
pub struct CertificateCollection<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<CertificateCollectionSchema>,
}

impl<B: Bmc> CertificateCollection<B> {
    /// Create a certificate collection from an advertised link.
    #[cfg(all(feature = "computer-systems", feature = "secure-boot"))]
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<CertificateCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::from)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// List the certificates in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching a certificate fails.
    pub async fn members(&self) -> Result<Vec<Certificate>, Error<B>> {
        let mut members = Vec::with_capacity(self.data.members.len());
        for member in &self.data.members {
            members.push(Certificate::from_nav(&self.bmc, member).await?);
        }
        Ok(members)
    }

    /// Install a certificate in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if creating or fetching the returned certificate fails.
    pub async fn create(
        &self,
        create: &CertificateCreate,
    ) -> Result<ModificationResponse<Certificate>, Error<B>> {
        self.bmc
            .as_ref()
            .create::<_, NavProperty<CertificateSchema>>(self.data.odata_id(), create)
            .await
            .map_err(Error::from)?
            .try_map_entity_async(|nav| async move { Certificate::from_nav(&self.bmc, &nav).await })
            .await
    }

    /// Get the raw schema data for this collection.
    #[must_use]
    pub fn raw(&self) -> Arc<CertificateCollectionSchema> {
        self.data.clone()
    }
}
