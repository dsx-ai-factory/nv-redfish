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

use crate::patch_support::JsonValue;
use crate::Error;
use nv_redfish_core::repair::decode;
use nv_redfish_core::Bmc;
use serde::Deserialize;

/// A collection member as it came, for the collection quirks that decide
/// on a member before it is deserialized.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Payload(JsonValue);

impl Payload {
    /// Apply function `f` to the payload and then try to deserialize to the
    /// target type. A reference is not patched: it names a document, it is
    /// not one.
    pub(crate) fn to_target<T, B, F>(&self, f: F) -> Result<T, Error<B>>
    where
        T: for<'de> Deserialize<'de>,
        B: Bmc,
        F: FnOnce(JsonValue) -> JsonValue,
    {
        let is_reference = self
            .0
            .as_object()
            .is_some_and(|obj| obj.len() == 1 && obj.contains_key("@odata.id"));
        if is_reference {
            self.parse()
        } else {
            decode(f(self.0.clone())).map_err(Error::Decode)
        }
    }

    pub(crate) fn parse<T, B>(&self) -> Result<T, Error<B>>
    where
        T: for<'de> Deserialize<'de>,
        B: Bmc,
    {
        decode(self.0.clone()).map_err(Error::Decode)
    }

    /// Whether `f` keeps the member.
    pub(crate) fn filter<F>(&self, f: F) -> bool
    where
        F: FnOnce(&JsonValue) -> bool,
    {
        f(&self.0)
    }
}
