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

//! Support NVIDIA `ProcessorMetrics` OEM extension.
//!
//! Unlike the other NVIDIA metric extensions this one has two concrete
//! shapes, chosen by `@odata.type`: `NvidiaGPUProcessorMetrics` adds
//! GPU counters on top of the properties every NVIDIA processor
//! reports, and plain `NvidiaProcessorMetrics` carries only the latter.
//! Each variant exposes its inherited properties directly.

use crate::oem::declares;
use crate::oem::nvidia::schema::nvidia_processor_metrics::v1_5_0::NvidiaProcessorMetrics as NvidiaProcessorMetricsSchema;
use crate::oem::nvidia::schema::nvidia_processor_metrics::NvidiaGpuProcessorMetrics as NvidiaGpuProcessorMetricsSchema;
use crate::oem::nvidia::OEM_KEY;
use crate::oem::oem_value;
use crate::schema::resource::Oem as ResourceOemSchema;
use crate::Error;
use nv_redfish_core::Bmc;
use serde::Deserialize as _;
use std::sync::Arc;

/// Namespace every shape in this family is declared under.
const NAMESPACE: &str = "NvidiaProcessorMetrics";
/// `@odata.type` name of the GPU shape.
const GPU_TYPE_NAME: &str = "NvidiaGPUProcessorMetrics";

/// NVIDIA extension of a processor's metrics.
pub enum NvidiaProcessorMetrics {
    /// GPU counters, reported by processors of type `GPU`.
    Gpu(Arc<NvidiaGpuProcessorMetricsSchema>),
    /// Only the properties common to every NVIDIA processor.
    Generic(Arc<NvidiaProcessorMetricsSchema>),
}

impl NvidiaProcessorMetrics {
    /// Read the extension out of a `ProcessorMetrics` OEM payload.
    ///
    /// Returns `Ok(None)` when the payload carries no NVIDIA object,
    /// including when it carries an explicit `null`.
    ///
    /// An `@odata.type` that is absent or names a shape this version
    /// does not know reads as [`Self::Generic`]. Reporting nothing
    /// would discard every metric on the resource over a property that
    /// only selects between shapes.
    pub(crate) fn new<B: Bmc>(oem: &ResourceOemSchema) -> Result<Option<Self>, Error<B>> {
        let Some(nvidia) = oem_value(oem, OEM_KEY) else {
            return Ok(None);
        };
        let this = if declares(nvidia, NAMESPACE, GPU_TYPE_NAME) {
            Self::Gpu(Arc::new(
                NvidiaGpuProcessorMetricsSchema::deserialize(nvidia).map_err(Error::Json)?,
            ))
        } else {
            Self::Generic(Arc::new(
                NvidiaProcessorMetricsSchema::deserialize(nvidia).map_err(Error::Json)?,
            ))
        };
        Ok(Some(this))
    }
}
