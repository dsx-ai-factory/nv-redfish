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
#[cfg(feature = "component-integrity")]
use serde_json::Value as JsonValue;

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
    #[cfg(feature = "component-integrity")]
    pub(crate) const fn new(data: Arc<CertificateSchema>) -> Self {
        Self { data }
    }

    /// Get the raw schema data for this certificate.
    #[must_use]
    pub fn raw(&self) -> Arc<CertificateSchema> {
        self.data.clone()
    }
}

/// Normalize the non-standard certificate-chain spelling used by H100 AMI.
#[cfg(feature = "component-integrity")]
pub(crate) fn normalize_pem_chain_type(mut value: JsonValue) -> JsonValue {
    if value.get("CertificateType").and_then(JsonValue::as_str) == Some("PEMChain") {
        value["CertificateType"] = JsonValue::String("PEMchain".to_string());
    }
    value
}
