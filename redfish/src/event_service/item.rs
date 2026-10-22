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

//! Redfish `EventDestination` - high-level wrapper.

use std::sync::Arc;

use crate::schema::event_destination::EventDestination as EventDestinationSchema;
use crate::schema::event_destination::SubscriptionType;
use crate::Error;
use crate::NvBmc;

use nv_redfish_core::odata::ODataType;
use nv_redfish_core::repair::Repair;
use nv_redfish_core::repair::RepairError;
use nv_redfish_core::Bmc;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataETag;
use nv_redfish_core::ODataId;

use serde::de;
use serde::Deserialize;
use serde::Deserializer;
use serde_json::Value;

/// Applies the absent SubscriptionType default before typed subscription decoding.
///
/// The repair edits only that key so generated fields and opaque OEM data remain intact.
struct SubscriptionDefaults;

impl Repair for SubscriptionDefaults {
    fn repair(&self, document: &mut Value) {
        // ID-only references must remain references until their resource is fetched.
        if ODataType::parse_from(document).is_none_or(|kind| kind.type_name != "EventDestination") {
            return;
        }

        if let Some(object) = document.as_object_mut() {
            // EventDestination defines RedfishEvent when SubscriptionType is absent.
            // Preserve explicit nulls and values; decoding still validates their types.
            object
                .entry("SubscriptionType")
                .or_insert_with(|| Value::from("RedfishEvent"));
        }
    }
}

// Collections and DELETE responses can contain expanded destinations. Default
// each destination before decoding, while retaining ID-only references and
// rejecting message bodies so the transport can classify them separately.
pub(super) struct DestinationNav(NavProperty<EventDestinationSchema>);

impl<'de> Deserialize<'de> for DestinationNav {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Repair the document without duplicating the generated model's fields
        // or dropping OEM data, then retain normal navigation-property validation.
        let mut document = Value::deserialize(deserializer)?;
        SubscriptionDefaults.repair(&mut document);

        NavProperty::deserialize(document)
            .map(Self)
            .map_err(de::Error::custom)
    }
}

impl EntityTypeRef for DestinationNav {
    fn odata_id(&self) -> &ODataId {
        self.0.odata_id()
    }

    fn etag(&self) -> Option<&ODataETag> {
        self.0.etag()
    }
}

/// Represents a Redfish `EventDestination`.
pub struct EventDestination<B: Bmc> {
    bmc: NvBmc<B>,
    data: Arc<EventDestinationSchema>,
}

impl<B: Bmc> EventDestination<B> {
    pub(super) async fn new(bmc: &NvBmc<B>, nav: &DestinationNav) -> Result<Self, Error<B>> {
        // Referenced destinations need defaults on fetch. Expanded destinations
        // arrive with defaults applied during collection or DELETE-response decoding.
        let data = if matches!(nav.0, NavProperty::Reference(_)) {
            bmc.as_ref()
                .get_repaired(nav.odata_id(), &SubscriptionDefaults)
                .await
                .map_err(|error| match error {
                    RepairError::Transport(error) => Error::from(error),
                    RepairError::Decode(error) => Error::Decode(error),
                })?
        } else {
            nav.0.get(bmc.as_ref()).await.map_err(Error::from)?
        };

        Ok(Self {
            bmc: bmc.clone(),
            data,
        })
    }

    /// Get the raw schema data for this event destination.
    #[must_use]
    pub fn raw(&self) -> Arc<EventDestinationSchema> {
        self.data.clone()
    }

    /// Get the subscription type, or `None` when the service reports `null`.
    /// An absent property defaults to `RedfishEvent` when reading subscriptions.
    #[must_use]
    pub fn subscription_type(&self) -> Option<SubscriptionType> {
        self.data.subscription_type
    }

    /// Delete this event destination.
    ///
    /// Returns one of the following modification outcomes:
    ///
    /// - `ModificationResponse::Entity` contains the event destination
    ///   returned by the server.
    /// - `ModificationResponse::Task` identifies an asynchronous operation.
    /// - `ModificationResponse::Message` reports synchronous success with a
    ///   message body.
    /// - `ModificationResponse::Empty` reports synchronous success without a
    ///   response body.
    ///
    /// A returned destination defaults an absent `SubscriptionType` to
    /// `RedfishEvent`, as subscription reads do.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its response cannot be decoded.
    pub async fn delete(&self) -> Result<ModificationResponse<Self>, Error<B>> {
        self.bmc
            .as_ref()
            .delete::<DestinationNav>(self.data.odata_id())
            .await
            .map_err(Error::from)?
            .try_map_entity_async(|response| async move { Self::new(&self.bmc, &response).await })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    #[test]
    fn destination_navigation_preserves_reference_only_payloads() {
        let response: DestinationNav = serde_json::from_value(json!({
            "@odata.id": "/redfish/v1/EventService/Subscriptions/1"
        }))
        .expect("destination reference");

        assert!(matches!(response.0, NavProperty::Reference(_)));

        assert_eq!(
            response.odata_id().to_string(),
            "/redfish/v1/EventService/Subscriptions/1"
        );
    }

    #[test]
    fn subscription_defaults_preserve_present_values_and_other_fields() {
        let document = json!({
            "@odata.type": "#EventDestination.v1_3_0.EventDestination",
            "@odata.id": "/redfish/v1/EventService/Subscriptions/1",
            "Context": "test context",
            "Oem": {"Example": {"Data": [1, 2, 3]}}
        });

        let mut absent = document.clone();
        let mut expected = document.clone();
        expected["SubscriptionType"] = json!("RedfishEvent");

        SubscriptionDefaults.repair(&mut absent);

        assert_eq!(absent, expected);

        for value in [json!(null), json!("SSE"), json!("FutureType"), json!(42)] {
            let mut present = document.clone();
            present["SubscriptionType"] = value;
            let expected = present.clone();

            SubscriptionDefaults.repair(&mut present);

            assert_eq!(present, expected);
        }

        for mut unrelated in [
            json!({"@odata.id": "/redfish/v1/EventService/Subscriptions/1"}),
            json!({"@odata.type": "#EventService.v1_0_0.EventService"}),
        ] {
            let expected = unrelated.clone();

            SubscriptionDefaults.repair(&mut unrelated);

            assert_eq!(unrelated, expected);
        }
    }
}
