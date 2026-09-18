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

//! AMI UpdateService request composition.

use crate::core::OemMultipartPart;
use crate::schema::resource::OemUpdate;
use crate::update_service::UpdateServiceUpdate;
use futures_util::io::Cursor;
use serde_json::Value;

#[doc(inline)]
pub use crate::oem::ami::schema::ami_update_service::AmiUpdateServiceUpdate;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_update_service::CpldFirmwareType;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_update_service::ImageType;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_update_service::OemParametersUpdate;
#[doc(inline)]
pub use crate::oem::ami::schema::ami_update_service::PreserveConfigurationUpdate;

const OEM_KEY: &str = "AMIUpdateService";
const OEM_PARAMETERS_PART: &str = "OemParameters";

/// Adds AMI OEM settings to a standard UpdateService update.
pub trait AmiUpdateServiceUpdateExt: Sized {
    /// Merge AMI OEM settings while preserving other OEM values.
    ///
    /// # Errors
    ///
    /// Returns an error if the AMI update cannot be serialized.
    fn with_oem_ami(self, ami_update: AmiUpdateServiceUpdate) -> Result<Self, serde_json::Error>;
}

impl AmiUpdateServiceUpdateExt for UpdateServiceUpdate {
    fn with_oem_ami(
        mut self,
        ami_update: AmiUpdateServiceUpdate,
    ) -> Result<Self, serde_json::Error> {
        let mut additional_properties = self
            .oem
            .take()
            .and_then(|oem| oem.additional_properties.as_object().cloned())
            .unwrap_or_default();
        additional_properties.insert(OEM_KEY.to_string(), serde_json::to_value(ami_update)?);
        self.oem = Some(OemUpdate {
            additional_properties: Value::Object(additional_properties),
        });
        Ok(self)
    }
}

/// Serialize typed AMI parameters as the multipart `OemParameters` JSON part.
///
/// # Errors
///
/// Returns an error if the parameters cannot be serialized.
pub fn oem_parameters_part(
    parameters: &OemParametersUpdate,
) -> Result<OemMultipartPart, serde_json::Error> {
    let data = serde_json::to_vec(parameters)?;
    let content_length = data.len() as u64;
    Ok(OemMultipartPart {
        name: OEM_PARAMETERS_PART.to_string(),
        reader: Box::pin(Cursor::new(data)),
        content_type: Some("application/json".to_string()),
        content_length: Some(content_length),
    })
}
