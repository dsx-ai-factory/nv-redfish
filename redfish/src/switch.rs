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

//! Switch entities and collections.

use crate::entity_link::EntityLink;
use crate::entity_link::FromLink;
use crate::hardware_id::HardwareIdRef;
use crate::hardware_id::Manufacturer as HardwareIdManufacturer;
use crate::hardware_id::Model as HardwareIdModel;
use crate::hardware_id::PartNumber as HardwareIdPartNumber;
use crate::hardware_id::SerialNumber as HardwareIdSerialNumber;
use crate::patch_support::CollectionWithPatch;
use crate::resource::PowerState;
#[doc(inline)]
pub use crate::schema::protocol::Protocol;
use crate::schema::resource::ResourceCollection;
use crate::schema::switch::Switch as SwitchSchema;
use crate::schema::switch_collection::SwitchCollection as SwitchCollectionSchema;
use crate::Error;
use crate::NvBmc;
use crate::ResourceProvidesStatus;
use crate::ResourceStatusSchema;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
use std::convert::identity;
use std::future::Future;
use std::sync::Arc;
use tagged_types::TaggedType;

#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaSwitch;
#[cfg(feature = "ports")]
use crate::port::PortCollection;

#[doc(hidden)]
pub enum SwitchTag {}

/// Switch manufacturer.
pub type Manufacturer<T> = HardwareIdManufacturer<T, SwitchTag>;

/// Switch model.
pub type Model<T> = HardwareIdModel<T, SwitchTag>;

/// Switch part number.
pub type PartNumber<T> = HardwareIdPartNumber<T, SwitchTag>;

/// Switch serial number.
pub type SerialNumber<T> = HardwareIdSerialNumber<T, SwitchTag>;

/// SKU of the switch.
pub type Sku<T> = TaggedType<T, SwitchSkuTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum SwitchSkuTag {}

/// Firmware version of the switch.
pub type FirmwareVersion<T> = TaggedType<T, SwitchFirmwareVersionTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum SwitchFirmwareVersionTag {}

/// Lazy link to a switch.
pub type SwitchLink<B> = EntityLink<B, SwitchSchema>;

/// Switch collection.
///
/// Provides functions to access collection members.
pub struct SwitchCollection<B: Bmc> {
    bmc: NvBmc<B>,
    collection: Arc<SwitchCollectionSchema>,
}

impl<B: Bmc> SwitchCollection<B> {
    /// Create a new switch collection handle.
    #[allow(dead_code)] // Used by fabric traversal when fabrics are enabled.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SwitchCollectionSchema>,
    ) -> Result<Self, Error<B>> {
        let collection = Self::expand_collection(bmc, nav, None, None).await?;
        Ok(Self {
            bmc: bmc.clone(),
            collection,
        })
    }

    /// List all switches in this collection.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching switch data fails.
    pub async fn members(&self) -> Result<Vec<Switch<B>>, Error<B>> {
        let mut members = Vec::new();
        for m in &self.collection.members {
            members.push(Switch::new(&self.bmc, m).await?);
        }
        Ok(members)
    }

    /// Return lazy links for the switches in this collection.
    ///
    /// Each link can be fetched independently, allowing callers to choose their own error-handling
    /// policy without changing the eager, all-or-nothing behavior of [`Self::members`].
    #[must_use]
    pub fn member_links(&self) -> Vec<SwitchLink<B>> {
        self.collection
            .members
            .iter()
            .map(|member| {
                SwitchLink::new(&self.bmc, NavProperty::new_reference(member.id().clone()))
            })
            .collect()
    }
}

impl<B: Bmc> CollectionWithPatch<SwitchCollectionSchema, SwitchSchema, B> for SwitchCollection<B> {
    fn convert_patched(
        base: ResourceCollection,
        members: Vec<NavProperty<SwitchSchema>>,
    ) -> SwitchCollectionSchema {
        SwitchCollectionSchema {
            odata_id: base.odata_id,
            odata_etag: base.odata_etag,
            odata_type: base.odata_type,
            settings_annotations: base.settings_annotations,
            description: base.description,
            name: base.name,
            oem: base.oem,
            members,
        }
    }
}

/// Represents a switch in a fabric.
///
/// Provides access to switch information and associated ports.
pub struct Switch<B: Bmc> {
    #[allow(dead_code)] // used if any feature enabled.
    bmc: NvBmc<B>,
    data: Arc<SwitchSchema>,
}

impl<B: Bmc> Switch<B> {
    /// Create a new switch handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<SwitchSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                bmc: bmc.clone(),
                data,
            })
    }

    /// Get the raw schema data for this switch.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<SwitchSchema> {
        self.data.clone()
    }

    /// Get hardware identifier of the switch.
    #[must_use]
    pub fn hardware_id(&self) -> HardwareIdRef<'_, SwitchTag> {
        HardwareIdRef {
            manufacturer: self
                .data
                .manufacturer
                .as_ref()
                .and_then(Option::as_deref)
                .map(Manufacturer::new),
            model: self
                .data
                .model
                .as_ref()
                .and_then(Option::as_deref)
                .map(Model::new),
            part_number: self
                .data
                .part_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(PartNumber::new),
            serial_number: self
                .data
                .serial_number
                .as_ref()
                .and_then(Option::as_deref)
                .map(SerialNumber::new),
        }
    }

    /// The SKU of this switch.
    #[must_use]
    pub fn sku(&self) -> Option<Sku<&str>> {
        self.data
            .sku
            .as_ref()
            .and_then(Option::as_deref)
            .map(Sku::new)
    }

    /// The firmware version of this switch.
    #[must_use]
    pub fn firmware_version(&self) -> Option<FirmwareVersion<&str>> {
        self.data
            .firmware_version
            .as_ref()
            .and_then(Option::as_deref)
            .map(FirmwareVersion::new)
    }

    /// The protocol being sent over this switch.
    #[must_use]
    pub fn switch_type(&self) -> Option<Protocol> {
        self.data.switch_type.and_then(identity)
    }

    /// The protocols this switch supports.
    #[must_use]
    pub fn supported_protocols(&self) -> Vec<Protocol> {
        self.data.supported_protocols.clone().unwrap_or_default()
    }

    /// The current power state of this switch.
    #[must_use]
    pub fn power_state(&self) -> Option<PowerState> {
        self.data.power_state.and_then(identity)
    }

    /// Whether this switch is enabled.
    #[must_use]
    pub fn enabled(&self) -> Option<bool> {
        self.data.enabled
    }

    /// Whether this switch is in a managed state.
    #[must_use]
    pub fn is_managed(&self) -> Option<bool> {
        self.data.is_managed.and_then(identity)
    }

    /// The number of lanes, phys or other physical transport links of this switch.
    #[must_use]
    pub fn total_switch_width(&self) -> Option<i64> {
        self.data.total_switch_width.and_then(identity)
    }

    /// The current internal bandwidth of this switch, in Gbit/s.
    #[must_use]
    pub fn current_bandwidth_gbps(&self) -> Option<f64> {
        self.data.current_bandwidth_gbps.and_then(identity)
    }

    /// The maximum internal bandwidth of this switch, in Gbit/s.
    #[must_use]
    pub fn max_bandwidth_gbps(&self) -> Option<f64> {
        self.data.max_bandwidth_gbps.and_then(identity)
    }

    /// Get ports for this switch.
    ///
    /// Returns `Ok(None)` when the ports link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching the port collection fails.
    #[cfg(feature = "ports")]
    pub async fn ports(&self) -> Result<Option<PortCollection<B>>, Error<B>> {
        if let Some(ports) = &self.data.ports {
            PortCollection::new(&self.bmc, ports).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get NVIDIA Switch OEM extension.
    ///
    /// Returns `Ok(None)` when the switch does not include `Oem.Nvidia`.
    ///
    /// # Errors
    ///
    /// Returns an error if parsing NVIDIA switch OEM data fails.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia(&self) -> Result<Option<NvidiaSwitch<B>>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), NvidiaSwitch::new)
    }
}

impl<B: Bmc> ResourceProvidesStatus for Switch<B> {
    fn resource_status_ref(&self) -> Option<&ResourceStatusSchema> {
        self.data.status.as_ref()
    }
}

impl<B: Bmc> FromLink<B> for Switch<B> {
    type Schema = SwitchSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}
