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

//! Lenovo AccountService OEM update support.

use crate::oem::lenovo::oem_update;
use crate::schema::account_service::AccountServiceUpdate;

#[doc(inline)]
pub use crate::oem::lenovo::schema::lenovo_account_service::LenovoAccountServicePropertiesUpdate as LenovoAccountServiceUpdate;

/// Adds Lenovo OEM settings to a standard AccountService update.
pub trait LenovoAccountServiceUpdateExt: Sized {
    /// Merge Lenovo OEM settings while preserving other OEM values.
    ///
    /// # Errors
    ///
    /// Returns an error if the Lenovo update cannot be serialized.
    fn with_oem_lenovo(
        self,
        lenovo_update: LenovoAccountServiceUpdate,
    ) -> Result<Self, serde_json::Error>;
}

impl LenovoAccountServiceUpdateExt for AccountServiceUpdate {
    fn with_oem_lenovo(
        mut self,
        lenovo_update: LenovoAccountServiceUpdate,
    ) -> Result<Self, serde_json::Error> {
        self.oem = Some(oem_update(self.oem.take(), &lenovo_update)?);
        Ok(self)
    }
}
