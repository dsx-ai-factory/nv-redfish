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

use crate::core::Bmc;
use crate::core::EntityTypeRef as _;
use crate::core::ModificationResponse;
use crate::core::NavProperty;
use crate::core::RedfishSettings as _;
use crate::hardware_id::HardwareIdRef;
use crate::hardware_id::Manufacturer as HardwareIdManufacturer;
use crate::hardware_id::Model as HardwareIdModel;
use crate::hardware_id::PartNumber as HardwareIdPartNumber;
use crate::hardware_id::SerialNumber as HardwareIdSerialNumber;
use crate::patch_support::Payload;
use crate::patch_support::ReadPatchFn;
use crate::resource::PowerState;
use crate::resource::ResetType;
use crate::schema::computer_system::ComputerSystem as ComputerSystemSchema;
use crate::Error;
use crate::NvBmc;

use std::convert::identity;
use std::sync::Arc;
use tagged_types::TaggedType;

pub use crate::schema::computer_system::BootUpdate;
pub use crate::schema::computer_system::ComputerSystemUpdate;
pub use crate::schema::computer_system::IpmiHostInterfaceUpdate;

#[cfg(feature = "bios")]
use crate::computer_system::Bios;
#[cfg(feature = "boot-options")]
use crate::computer_system::BootOptionCollection;
#[cfg(feature = "memory")]
use crate::computer_system::Memory;
#[cfg(feature = "processors")]
use crate::computer_system::Processor;
#[cfg(feature = "secure-boot")]
use crate::computer_system::SecureBoot;
#[cfg(feature = "storages")]
use crate::computer_system::Storage;
#[cfg(feature = "ethernet-interfaces")]
use crate::ethernet_interface::EthernetInterfaceCollection;
#[cfg(feature = "log-services")]
use crate::log_service::LogService;
#[cfg(feature = "oem-lenovo")]
use crate::oem::lenovo::computer_system::LenovoComputerSystem;
#[cfg(feature = "oem-lenovo")]
use crate::oem::lenovo::LenovoComputerSystemActions;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaComputerSystem;
#[cfg(feature = "oem-supermicro")]
use crate::oem::supermicro::SupermicroComputerSystem;
#[cfg(feature = "oem-supermicro")]
use crate::oem::supermicro::SupermicroComputerSystemActions;

#[doc(hidden)]
pub enum ComputerSystemTag {}

/// Computer system manufacturer.
pub type Manufacturer<T> = HardwareIdManufacturer<T, ComputerSystemTag>;

/// Computer system model.
pub type Model<T> = HardwareIdModel<T, ComputerSystemTag>;

/// Computer system part number.
pub type PartNumber<T> = HardwareIdPartNumber<T, ComputerSystemTag>;

/// Computer system serial number.
pub type SerialNumber<T> = HardwareIdSerialNumber<T, ComputerSystemTag>;

/// Computer system SKU.
pub type Sku<T> = TaggedType<T, ComputerSystemSkuTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, FromStr, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum ComputerSystemSkuTag {}

/// `BootOptionReference` type represent boot order of the `ComputerSystem`.
pub type BootOptionReference<T> = TaggedType<T, BootOptionReferenceTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[transparent(Debug, Display, FromStr, Serialize, Deserialize)]
#[capability(inner_access, cloned)]
pub enum BootOptionReferenceTag {}

/// Represents a computer system in the BMC.
///
/// Provides access to system information and sub-resources such as processors.
pub struct ComputerSystem<B: Bmc> {
    #[allow(dead_code)] // feature-enabled...
    bmc: NvBmc<B>,
    data: Arc<ComputerSystemSchema>,
    read_patch_fn: Option<ReadPatchFn>,
}

impl<B: Bmc> ComputerSystem<B> {
    /// Create a new computer system handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ComputerSystemSchema>,
        read_patch_fn: Option<&ReadPatchFn>,
    ) -> Result<Self, Error<B>> {
        let read_patch_fn = read_patch_fn.cloned();
        if let Some(read_patch_fn) = read_patch_fn.as_ref() {
            Payload::get(bmc.as_ref(), nav, read_patch_fn.as_ref()).await
        } else {
            nav.get(bmc.as_ref()).await.map_err(Error::Bmc)
        }
        .map(|data| Self {
            bmc: bmc.clone(),
            data,
            read_patch_fn,
        })
    }

    /// Get the raw schema data for this computer system.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<ComputerSystemSchema> {
        self.data.clone()
    }

    /// Get hardware identifier of the network adapter.
    #[must_use]
    pub fn hardware_id(&self) -> HardwareIdRef<'_, ComputerSystemTag> {
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

    /// The manufacturer SKU for this system.
    #[must_use]
    pub fn sku(&self) -> Option<Sku<&str>> {
        self.data
            .sku
            .as_ref()
            .and_then(Option::as_ref)
            .map(String::as_str)
            .map(Sku::new)
    }

    /// Power state of this system.
    #[must_use]
    pub fn power_state(&self) -> Option<PowerState> {
        self.data.power_state.and_then(identity)
    }

    /// Whether the system's in-band IPMI host interface is enabled.
    #[must_use]
    pub fn ipmi_host_interface_enabled(&self) -> Option<bool> {
        self.data
            .ipmi_host_interface
            .as_ref()
            .and_then(|interface| interface.service_enabled)
    }

    /// Enable or disable the in-band IPMI host interface.
    ///
    /// # Errors
    ///
    /// Returns an error if updating or fetching the returned system fails.
    pub async fn set_ipmi_host_interface_enabled(
        &self,
        enabled: bool,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update(
            &ComputerSystemUpdate::builder()
                .with_ipmi_host_interface(
                    IpmiHostInterfaceUpdate::builder()
                        .with_service_enabled(enabled)
                        .build(),
                )
                .build(),
        )
        .await
    }

    /// Update this computer system.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the computer system fails.
    pub async fn update(
        &self,
        update: &ComputerSystemUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.update_at(self.data.odata_id(), self.data.etag(), update)
            .await
    }

    /// Reset this computer system.
    ///
    /// # Errors
    ///
    /// Returns an error if the system does not support the `Reset` action or
    /// if invoking the action fails.
    pub async fn reset(
        &self,
        reset_type: Option<ResetType>,
    ) -> Result<ModificationResponse<()>, Error<B>>
    where
        B::Error: nv_redfish_core::ActionError,
    {
        let actions = self
            .data
            .actions
            .as_ref()
            .ok_or(Error::ActionNotAvailable)?;

        if actions.reset.is_none() {
            return Err(Error::ActionNotAvailable);
        }

        actions
            .reset(self.bmc.as_ref(), reset_type)
            .await
            .map_err(Error::Bmc)
    }

    /// An array of `BootOptionReference` strings that represent the persistent boot order for with this
    /// computer system.
    #[must_use]
    pub fn boot_order(&self) -> Option<Vec<BootOptionReference<&str>>> {
        self.data
            .as_ref()
            .boot
            .as_ref()
            .and_then(|boot| boot.boot_order.as_ref().and_then(Option::as_ref))
            .map(|v| {
                v.iter()
                    .map(String::as_str)
                    .map(BootOptionReference::new)
                    .collect::<Vec<_>>()
            })
    }

    /// Update the persistent boot order for this computer system.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the updated computer system.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// # Errors
    ///
    /// Returns an error if updating the system fails.
    pub async fn set_boot_order(
        &self,
        boot_order: Vec<BootOptionReference<String>>,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        let update = ComputerSystemUpdate::builder()
            .with_boot(
                BootUpdate::builder()
                    .with_boot_order(
                        boot_order
                            .into_iter()
                            .map(|reference| reference.inner().clone())
                            .collect(),
                    )
                    .build(),
            )
            .build();

        let settings = self.data.settings_object();
        let update_odata = settings
            .as_ref()
            .map_or_else(|| self.data.odata_id(), |settings| settings.odata_id());
        self.update_at(update_odata, None, &update).await
    }

    /// Update the computer system resource at the supplied identifier.
    async fn update_at(
        &self,
        odata_id: &nv_redfish_core::ODataId,
        etag: Option<&nv_redfish_core::ODataETag>,
        update: &ComputerSystemUpdate,
    ) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .update::<_, NavProperty<ComputerSystemSchema>>(odata_id, etag, update)
            .await
            .map_err(Error::Bmc)?
            .try_map_entity_async(|nav| async move {
                Self::new(&self.bmc, &nav, self.read_patch_fn.as_ref()).await
            })
            .await
    }

    /// Bios associated with this system.
    ///
    /// Fetches the BIOS settings. Returns `Ok(None)` when the BIOS link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching BIOS data fails.
    #[cfg(feature = "bios")]
    pub async fn bios(&self) -> Result<Option<Bios<B>>, Error<B>> {
        if let Some(bios_ref) = &self.data.bios {
            Bios::new(&self.bmc, bios_ref).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get processors associated with this system.
    ///
    /// Fetches the processor collection and returns a list of [`Processor`] handles.
    /// Returns `Ok(None)` when the processors link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching processor data fails.
    #[cfg(feature = "processors")]
    pub async fn processors(&self) -> Result<Option<Vec<Processor<B>>>, Error<B>> {
        if let Some(processors_ref) = &self.data.processors {
            let processors_collection = self.bmc.expand_property(processors_ref).await?;

            let mut processors = Vec::new();
            for m in &processors_collection.members {
                processors.push(Processor::new(&self.bmc, m).await?);
            }

            Ok(Some(processors))
        } else {
            Ok(None)
        }
    }

    /// Get secure boot resource associated with this system.
    ///
    /// Returns `Ok(None)` when the secure boot link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching secure boot data fails.
    #[cfg(feature = "secure-boot")]
    pub async fn secure_boot(&self) -> Result<Option<SecureBoot<B>>, Error<B>> {
        if let Some(secure_boot_ref) = &self.data.secure_boot {
            SecureBoot::new(&self.bmc, secure_boot_ref).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get storage controllers associated with this system.
    ///
    /// Fetches the storage collection and returns a list of [`Storage`] handles.
    /// Returns `Ok(None)` when the storage link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching storage data fails.
    #[cfg(feature = "storages")]
    pub async fn storage_controllers(&self) -> Result<Option<Vec<Storage<B>>>, Error<B>> {
        if let Some(storage_ref) = &self.data.storage {
            use crate::computer_system::storage::StorageCollection;
            let storage_collection = StorageCollection::new(&self.bmc, storage_ref).await?;
            storage_collection.members().await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get memory modules associated with this system.
    ///
    /// Fetches the memory collection and returns a list of [`Memory`] handles.
    /// Returns `Ok(None)` when the memory link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching memory data fails.
    #[cfg(feature = "memory")]
    pub async fn memory_modules(&self) -> Result<Option<Vec<Memory<B>>>, Error<B>> {
        if let Some(memory_ref) = &self.data.memory {
            let memory_collection = self.bmc.expand_property(memory_ref).await?;

            let mut memory_modules = Vec::new();
            for m in &memory_collection.members {
                memory_modules.push(Memory::new(&self.bmc, m).await?);
            }

            Ok(Some(memory_modules))
        } else {
            Ok(None)
        }
    }

    /// Get log services for this computer system.
    ///
    /// Returns `Ok(None)` when the log services link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching log service data fails.
    #[cfg(feature = "log-services")]
    pub async fn log_services(&self) -> Result<Option<Vec<LogService<B>>>, Error<B>> {
        if let Some(log_services_ref) = &self.data.log_services {
            let log_services_collection = log_services_ref
                .get(self.bmc.as_ref())
                .await
                .map_err(Error::Bmc)?;

            let mut log_services = Vec::new();
            for m in &log_services_collection.members {
                log_services.push(LogService::new(&self.bmc, m).await?);
            }

            Ok(Some(log_services))
        } else {
            Ok(None)
        }
    }

    /// Get ethernet interfaces for this computer system.
    ///
    /// Returns `Ok(None)` when the ethernet interfaces link is absent.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching ethernet interface data fails.
    #[cfg(feature = "ethernet-interfaces")]
    pub async fn ethernet_interfaces(
        &self,
    ) -> Result<Option<EthernetInterfaceCollection<B>>, Error<B>> {
        if let Some(p) = &self.data.ethernet_interfaces {
            EthernetInterfaceCollection::new(&self.bmc, p)
                .await
                .map(Some)
        } else {
            Ok(None)
        }
    }

    /// Get collection of the UEFI boot options associated with this computer system.
    ///
    /// Returns `Ok(None)` when boot options are not exposed.
    ///
    /// # Errors
    ///
    /// Returns an error if fetching boot options data fails.
    #[cfg(feature = "boot-options")]
    pub async fn boot_options(&self) -> Result<Option<BootOptionCollection<B>>, Error<B>> {
        if let Some(p) = &self
            .data
            .boot
            .as_ref()
            .and_then(|v| v.boot_options.as_ref())
        {
            BootOptionCollection::new(&self.bmc, p).await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// NVIDIA OEM extension
    ///
    /// Returns `Ok(None)` when the system does not include NVIDIA OEM extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM data parsing/fetching fails.
    #[cfg(feature = "oem-nvidia")]
    pub async fn oem_nvidia(&self) -> Result<Option<NvidiaComputerSystem<B>>, Error<B>> {
        if let Some(oem) = self.data.oem.as_ref() {
            NvidiaComputerSystem::new(&self.bmc, oem).await
        } else {
            Ok(None)
        }
    }

    /// Lenovo OEM extension
    ///
    /// Returns `Ok(None)` when the system does not include Lenovo OEM extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if Lenovo OEM data parsing fails.
    #[cfg(feature = "oem-lenovo")]
    pub fn oem_lenovo(&self) -> Result<Option<LenovoComputerSystem<B>>, Error<B>> {
        LenovoComputerSystem::new(&self.bmc, &self.data, self.read_patch_fn.as_ref())
    }

    /// Get the Lenovo OEM actions advertised by this computer system.
    ///
    /// Returns `Ok(None)` when the system has no OEM actions object.
    ///
    /// # Errors
    ///
    /// Returns an error if Lenovo OEM actions cannot be parsed.
    #[cfg(feature = "oem-lenovo")]
    pub fn oem_lenovo_actions(&self) -> Result<Option<LenovoComputerSystemActions<B>>, Error<B>> {
        self.data
            .actions
            .as_ref()
            .and_then(|actions| actions.oem.as_ref())
            .map(|actions| LenovoComputerSystemActions::new(&self.bmc, actions))
            .transpose()
    }

    /// Get the Supermicro OEM properties advertised by this computer system.
    ///
    /// Returns `Ok(None)` when the system does not advertise `Oem.Supermicro`.
    ///
    /// # Errors
    ///
    /// Returns an error if Supermicro OEM data cannot be parsed.
    #[cfg(feature = "oem-supermicro")]
    pub fn oem_supermicro(&self) -> Result<Option<SupermicroComputerSystem<B>>, Error<B>> {
        SupermicroComputerSystem::new(&self.bmc, &self.data)
    }

    /// Get the advertised Supermicro OEM system actions.
    ///
    /// Returns `Ok(None)` when the system does not advertise the Supermicro
    /// AC-cycle action.
    ///
    /// # Errors
    ///
    /// Returns an error if Supermicro OEM actions cannot be parsed.
    #[cfg(feature = "oem-supermicro")]
    pub fn oem_supermicro_actions(
        &self,
    ) -> Result<Option<SupermicroComputerSystemActions<B>>, Error<B>> {
        self.data
            .actions
            .as_ref()
            .and_then(|actions| actions.oem.as_ref())
            .map_or_else(
                || Ok(None),
                |actions| SupermicroComputerSystemActions::new(&self.bmc, actions),
            )
    }
}
