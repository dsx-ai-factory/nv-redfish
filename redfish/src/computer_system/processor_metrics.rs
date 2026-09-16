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

//! Performance and state metrics of a processor.

#[cfg(feature = "impl-entity-link")]
use crate::entity_link::FromLink;
use crate::schema::processor_metrics::ProcessorMetrics as ProcessorMetricsSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
#[cfg(feature = "impl-entity-link")]
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::NvidiaProcessorMetrics;

/// Represents the metrics of a processor in the BMC.
pub struct ProcessorMetrics<B: Bmc> {
    data: Arc<ProcessorMetricsSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> ProcessorMetrics<B> {
    /// Create a new processor metrics handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<ProcessorMetricsSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for these processor metrics.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<ProcessorMetricsSchema> {
        self.data.clone()
    }

    /// NVIDIA OEM extension.
    ///
    /// Returns `Ok(None)` when the metrics do not include NVIDIA OEM
    /// extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM data parsing fails.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia(&self) -> Result<Option<NvidiaProcessorMetrics>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), NvidiaProcessorMetrics::new)
    }
}

#[cfg(feature = "impl-entity-link")]
impl<B: Bmc> FromLink<B> for ProcessorMetrics<B> {
    type Schema = ProcessorMetricsSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}
