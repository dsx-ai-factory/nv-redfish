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

//! A report of collected metric readings.

#[cfg(feature = "impl-entity-link")]
use crate::entity_link::FromLink;
use crate::schema::metric_report::MetricReport as MetricReportSchema;
use crate::Error;
use crate::NvBmc;
use nv_redfish_core::Bmc;
use nv_redfish_core::NavProperty;
#[cfg(feature = "impl-entity-link")]
use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;

#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::schema::nvidia_metric_report::NvidiaMetricReport;
#[cfg(feature = "oem-nvidia")]
use crate::oem::nvidia::OEM_KEY;
#[cfg(feature = "oem-nvidia")]
use crate::oem::oem_object;

/// Represents a metric report in the BMC.
pub struct MetricReport<B: Bmc> {
    data: Arc<MetricReportSchema>,
    _marker: PhantomData<B>,
}

impl<B: Bmc> MetricReport<B> {
    /// Create a new metric report handle.
    pub(crate) async fn new(
        bmc: &NvBmc<B>,
        nav: &NavProperty<MetricReportSchema>,
    ) -> Result<Self, Error<B>> {
        nav.get(bmc.as_ref())
            .await
            .map_err(Error::Bmc)
            .map(|data| Self {
                data,
                _marker: PhantomData,
            })
    }

    /// Get the raw schema data for this metric report.
    ///
    /// Returns an `Arc` to the underlying schema, allowing cheap cloning
    /// and sharing of the data.
    #[must_use]
    pub fn raw(&self) -> Arc<MetricReportSchema> {
        self.data.clone()
    }

    /// NVIDIA OEM extension.
    ///
    /// Returns `Ok(None)` when the report does not include NVIDIA OEM
    /// extension data.
    ///
    /// # Errors
    ///
    /// Returns an error if NVIDIA OEM data parsing fails.
    #[cfg(feature = "oem-nvidia")]
    pub fn oem_nvidia(&self) -> Result<Option<Arc<NvidiaMetricReport>>, Error<B>> {
        self.data
            .oem
            .as_ref()
            .map_or_else(|| Ok(None), |oem| oem_object(oem, OEM_KEY))
    }
}

#[cfg(feature = "impl-entity-link")]
impl<B: Bmc> FromLink<B> for MetricReport<B> {
    type Schema = MetricReportSchema;

    fn from_link(
        bmc: &NvBmc<B>,
        nav: &NavProperty<Self::Schema>,
    ) -> impl Future<Output = Result<Self, Error<B>>> + Send {
        Self::new(bmc, nav)
    }
}
