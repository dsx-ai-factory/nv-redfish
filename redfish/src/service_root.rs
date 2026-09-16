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

use std::sync::Arc;

use crate::bmc_quirks::BmcQuirks;
use crate::core::Bmc;
use crate::core::NavProperty;
use crate::core::ODataId;
use crate::schema::service_root::ServiceRoot as SchemaServiceRoot;
use crate::Error;
use crate::NvBmc;
use crate::ProtocolFeatures;
use crate::Resource;
use crate::ResourceSchema;

use tagged_types::TaggedType;

#[cfg(feature = "accounts")]
use crate::account::AccountService;
#[cfg(feature = "chassis")]
use crate::chassis::ChassisCollection;
#[cfg(feature = "chassis")]
use crate::chassis::ChassisLink;
#[cfg(feature = "computer-systems")]
use crate::computer_system::SystemCollection;
#[cfg(feature = "event-service")]
use crate::event_service::EventService;
#[cfg(feature = "managers")]
use crate::manager::ManagerCollection;
#[cfg(feature = "oem-ami")]
use crate::oem::ami::AmiServiceRoot;
#[cfg(feature = "oem-hpe")]
use crate::oem::hpe::HpeiLoServiceExt;
#[cfg(feature = "power-equipment")]
use crate::power_equipment::PowerEquipment;
#[cfg(feature = "session-service")]
use crate::session_service::SessionService;
#[cfg(feature = "task-service")]
use crate::task_service::TaskService;
#[cfg(feature = "telemetry-service")]
use crate::telemetry_service::TelemetryService;
#[cfg(feature = "update-service")]
use crate::update_service::UpdateService;

/// The vendor or manufacturer associated with Redfish service.
pub type Vendor<T> = TaggedType<T, VendorTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum VendorTag {}

/// The product associated with Redfish service..
pub type Product<T> = TaggedType<T, ProductTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum ProductTag {}

/// The version of Redfish schema.
pub type RedfishVersion<'a> = TaggedType<&'a str, RedfishVersionTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum RedfishVersionTag {}

/// Represents `ServiceRoot` in the BMC model.
pub struct ServiceRoot<B: Bmc> {
    /// Content of the root.
    pub root: Arc<SchemaServiceRoot>,
    #[allow(dead_code)] // feature-enabled field
    bmc: NvBmc<B>,
}

// Implement Clone manually to avoid requiring B: Clone; cloning only
// needs Arc/NvBmc clones.
impl<B: Bmc> Clone for ServiceRoot<B> {
    fn clone(&self) -> Self {
        Self {
            root: self.root.clone(),
            bmc: self.bmc.clone(),
        }
    }
}

impl<B: Bmc> ServiceRoot<B> {
    /// Create a new service root.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving the root path via Redfish fails.
    pub async fn new(bmc: Arc<B>) -> Result<Self, Error<B>> {
        let root = NavProperty::<SchemaServiceRoot>::new_reference(ODataId::service_root())
            .get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)?;
        let quirks = BmcQuirks::new(&root, bmc.as_ref()).await?;
        let mut protocol_features = root
            .protocol_features_supported
            .as_ref()
            .map(ProtocolFeatures::new)
            .unwrap_or_default();

        if quirks.expand_is_not_working_properly() {
            protocol_features.expand.expand_all = false;
            protocol_features.expand.no_links = false;
        }

        let bmc = NvBmc::new(bmc, protocol_features, quirks);
        Ok(Self { root, bmc })
    }

    /// Replace BMC in this root.
    #[must_use]
    pub fn replace_bmc(self, bmc: Arc<B>) -> Self {
        let root = self.root;
        let bmc = self.bmc.replace_bmc(bmc);
        Self { root, bmc }
    }

    /// Restrict usage of expand.
    #[must_use]
    pub fn restrict_expand(self) -> Self {
        let root = self.root;
        let bmc = self.bmc.restrict_expand();
        Self { root, bmc }
    }

    /// The vendor or manufacturer associated with this Redfish service.
    pub fn vendor(&self) -> Option<Vendor<&str>> {
        self.root
            .vendor
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(Vendor::new)
    }

    /// The product associated with this Redfish service.
    pub fn product(&self) -> Option<Product<&str>> {
        self.root
            .product
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(Product::new)
    }

    /// The vendor or manufacturer associated with this Redfish service.
    pub fn redfish_version(&self) -> Option<RedfishVersion<'_>> {
        self.root
            .redfish_version
            .as_deref()
            .map(RedfishVersion::new)
    }

    /// Get the account service belonging to the BMC.
    ///
    /// Returns `Ok(None)` when the BMC does not expose AccountService.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving account service data fails.
    #[cfg(feature = "accounts")]
    pub async fn account_service(&self) -> Result<Option<AccountService<B>>, Error<B>> {
        AccountService::new(&self.bmc, self).await
    }

    /// Get chassis collection in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose Chassis.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving chassis collection data fails.
    #[cfg(feature = "chassis")]
    pub async fn chassis(&self) -> Result<Option<ChassisCollection<B>>, Error<B>> {
        ChassisCollection::new(&self.bmc, self).await
    }

    /// Get chassis links
    ///
    /// Returns `Ok(None)` when the BMC does not expose Chassis.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving chassis collection data fails.
    #[cfg(feature = "chassis")]
    pub async fn chassis_links(&self) -> Result<Option<Vec<ChassisLink<B>>>, Error<B>> {
        if let Some(collection) = &self.root.chassis {
            let collection = collection
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?;
            Ok(Some(
                collection
                    .members
                    .iter()
                    .map(|member| {
                        ChassisLink::new(&self.bmc, NavProperty::new_reference(member.id().clone()))
                    })
                    .collect(),
            ))
        } else {
            Ok(None)
        }
    }

    /// Get computer system collection in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose Systems.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving system collection data fails.
    #[cfg(feature = "computer-systems")]
    pub async fn systems(&self) -> Result<Option<SystemCollection<B>>, Error<B>> {
        SystemCollection::new(&self.bmc, self).await
    }

    /// Get update service in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose UpdateService.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving update service data fails.
    #[cfg(feature = "update-service")]
    pub async fn update_service(&self) -> Result<Option<UpdateService<B>>, Error<B>> {
        UpdateService::new(&self.bmc, self).await
    }

    /// Get task service in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose TaskService.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving task service data fails.
    #[cfg(feature = "task-service")]
    pub async fn task_service(&self) -> Result<Option<TaskService<B>>, Error<B>> {
        TaskService::new(&self.bmc, self).await
    }

    /// Get event service in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose EventService.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving event service data fails.
    #[cfg(feature = "event-service")]
    pub async fn event_service(&self) -> Result<Option<EventService<B>>, Error<B>> {
        EventService::new(&self.bmc, self).await
    }

    /// Get telemetry service in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose TelemetryService.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving telemetry service data fails.
    #[cfg(feature = "telemetry-service")]
    pub async fn telemetry_service(&self) -> Result<Option<TelemetryService<B>>, Error<B>> {
        TelemetryService::new(&self.bmc, self).await
    }

    /// Get session service in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose SessionService.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving session service data fails.
    #[cfg(feature = "session-service")]
    pub async fn session_service(&self) -> Result<Option<SessionService<B>>, Error<B>> {
        SessionService::new(&self.bmc, self).await
    }

    /// Get manager collection in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose Managers.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving manager collection data fails.
    #[cfg(feature = "managers")]
    pub async fn managers(&self) -> Result<Option<ManagerCollection<B>>, Error<B>> {
        ManagerCollection::new(&self.bmc, self).await
    }

    /// Get power equipment in BMC
    ///
    /// Returns `Ok(None)` when the BMC does not expose PowerEquipment.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving power equipment data fails.
    #[cfg(feature = "power-equipment")]
    pub async fn power_equipment(&self) -> Result<Option<PowerEquipment<B>>, Error<B>> {
        PowerEquipment::new(&self.bmc, self).await
    }

    /// Get HPE OEM extension in service root
    ///
    /// Returns `Ok(None)` when the BMC does not expose HPE extension.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving manager collection data fails.
    #[cfg(feature = "oem-hpe")]
    pub fn oem_hpe_ilo_service_ext(&self) -> Result<Option<HpeiLoServiceExt<B>>, Error<B>> {
        HpeiLoServiceExt::new(&self.root)
    }

    /// Get AMI OEM extension in service root
    ///
    /// Returns `Ok(None)` when the BMC does not expose AMI extension.
    ///
    /// # Errors
    ///
    /// Returns error if retrieving service root data fails.
    #[cfg(feature = "oem-ami")]
    pub fn oem_ami_service_root(&self) -> Result<Option<AmiServiceRoot<B>>, Error<B>> {
        AmiServiceRoot::new(&self.bmc, &self.root)
    }
}

impl<B: Bmc> Resource for ServiceRoot<B> {
    fn resource_ref(&self) -> &ResourceSchema {
        &self.root.as_ref().base
    }
}
