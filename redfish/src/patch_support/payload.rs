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
use nv_redfish_core::Bmc;
use serde::Deserialize;

#[cfg(any(feature = "patch-payload-get", feature = "patch-payload-update"))]
use nv_redfish_core::EntityTypeRef;
#[cfg(any(feature = "patch-payload-get", feature = "patch-payload-update"))]
use nv_redfish_core::Expandable;
#[cfg(any(feature = "patch-payload-get", feature = "patch-payload-update"))]
use nv_redfish_core::ODataETag;
#[cfg(any(feature = "patch-payload-get", feature = "patch-payload-update"))]
use nv_redfish_core::ODataId;
#[cfg(any(feature = "patch-payload-get", feature = "patch-payload-update"))]
use serde::Deserializer;

#[cfg(feature = "patch-payload-get")]
use nv_redfish_core::NavProperty;
#[cfg(feature = "patch-payload-get")]
use std::sync::Arc;

#[cfg(feature = "patch-payload-update")]
use crate::patch_support::ReadPatchFn;
#[cfg(feature = "patch-payload-update")]
use crate::NvBmc;
#[cfg(feature = "patch-payload-update")]
use nv_redfish_core::ModificationResponse;
#[cfg(feature = "patch-payload-update")]
use nv_redfish_core::Updatable;
#[cfg(feature = "patch-payload-update")]
use serde::Serialize;

#[cfg(feature = "patch-payload-update")]
pub trait UpdateWithPatch<T, V, B>
where
    V: Serialize + Send + Sync,
    T: Updatable<V>,
    B: Bmc,
{
    fn entity_ref(&self) -> &T;
    fn patch(&self) -> Option<&ReadPatchFn>;
    fn bmc(&self) -> &NvBmc<B>;

    async fn update_with_patch(&self, update: &V) -> Result<ModificationResponse<T>, Error<B>> {
        let entity = self.entity_ref();
        if let Some(patch_fn) = self.patch() {
            self.bmc()
                .update::<V, Payload>(entity.odata_id(), entity.etag(), update)
                .await?
                .try_map_entity(|payload| payload.to_target::<T, B, _>(patch_fn.as_ref()))
        } else {
            self.bmc()
                .update(entity.odata_id(), entity.etag(), update)
                .await
        }
    }
}

/// Support payload patching.
///
/// This struct supports deserialization from any JSON payload and
/// provides a method to apply a patch and then deserialize to the
/// target type.
#[derive(Deserialize)]
#[serde(transparent)]
pub struct Payload(JsonValue);

impl Payload {
    #[cfg(feature = "patch-payload-get")]
    pub(crate) async fn get<T, B, F>(
        bmc: &B,
        nav: &NavProperty<T>,
        f: F,
    ) -> Result<Arc<T>, Error<B>>
    where
        T: EntityTypeRef + for<'de> Deserialize<'de> + 'static,
        B: Bmc,
        F: FnOnce(JsonValue) -> JsonValue,
    {
        match nav {
            NavProperty::Expanded(_) => nav.get(bmc).await.map_err(Error::Bmc),
            NavProperty::Reference(_) => {
                let getter = NavProperty::<Getter>::new_reference(nav.id().clone());
                let v = getter.get(bmc).await.map_err(Error::Bmc)?;
                v.payload.to_target(f).map(Arc::new)
            }
        }
    }

    /// Apply function `f` to the payload and then try to deserialize to the
    /// target type.
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
            // Do not apply patches to the references.
            serde_json::from_value(self.0.clone()).map_err(Error::Json)
        } else {
            serde_json::from_value(f(self.0.clone())).map_err(Error::Json)
        }
    }

    #[cfg(feature = "patch-collection")]
    pub(crate) fn parse<T, B>(&self) -> Result<T, Error<B>>
    where
        T: for<'de> Deserialize<'de>,
        B: Bmc,
    {
        serde_json::from_value(self.0.clone()).map_err(Error::Json)
    }

    /// Apply function `f` to the payload and then try to deserialize to the
    /// target type.
    #[cfg(feature = "patch-collection")]
    pub(crate) fn filter<F>(&self, f: F) -> bool
    where
        F: FnOnce(&JsonValue) -> bool,
    {
        f(&self.0)
    }
}

#[cfg(feature = "patch-payload-get")]
struct Getter {
    id: ODataId,
    payload: Payload,
}

#[cfg(feature = "patch-payload-get")]
impl EntityTypeRef for Getter {
    fn odata_id(&self) -> &ODataId {
        &self.id
    }
    fn etag(&self) -> Option<&ODataETag> {
        None
    }
}

#[cfg(feature = "patch-payload-get")]
impl Expandable for Getter {}

#[cfg(feature = "patch-payload-get")]
impl<'de> Deserialize<'de> for Getter {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self {
            id: String::new().into(),
            payload: Payload::deserialize(deserializer)?,
        })
    }
}
