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

//! Redfish Action primitives
//!
//! The [`Action<T, R>`] type corresponds to the inner object found under the
//! Redfish `Actions` section for a specific action (for example,
//! `"#ComputerSystem.Reset"`). It captures the endpoint used to invoke the
//! action via its `target` field. The type parameters are:
//! - `T`: request parameters payload type (sent as the POST body when running the action)
//! - `R`: response type returned by the BMC for that action
//!
//! Only the `target` field is retained when serializing or deserializing.
//! Additional metadata (such as `...@Redfish.AllowableValues`) is ignored by
//! this type and may be used by higher layers.
//!
//! Example: how an action appears in a Redfish resource and which part maps to [`Action`]
//!
//! ```json
//! {
//!   "Actions": {
//!     "#ComputerSystem.Reset": {
//!       "target": "/redfish/v1/Systems/1/Actions/ComputerSystem.Reset",
//!       "ResetType@Redfish.AllowableValues": [
//!         "On",
//!         "GracefulRestart",
//!         "ForceRestart"
//!       ]
//!     }
//!   }
//! }
//! ```
//!
//! The [`Action<T, R>`] value corresponds to the inner object of
//! `"#ComputerSystem.Reset"` and retains the `target` field only.
//!

use crate::Bmc;
use crate::ModificationResponse;
use core::fmt::Debug;
use core::fmt::Display;
use core::fmt::Formatter;
use core::fmt::Result as FmtResult;
use serde::Deserialize;
use serde::Serialize;
use std::marker::PhantomData;

/// URI reference for the `target` field of an action.
///
/// The [`Bmc`] implementation resolves this value when the action is run and
/// may reject values that violate its outbound request policy before transport.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct ActionTarget(String);

impl ActionTarget {
    /// Creates new `ActionTarget`.
    #[must_use]
    pub const fn new(v: String) -> Self {
        Self(v)
    }

    /// Returns the action target URI reference.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Display for ActionTarget {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        Display::fmt(&self.0, f)
    }
}

/// Defines a serializable and deserializable Action. It is almost always a
/// member of the `Actions` struct in different parts of the Redfish object tree.
///
/// `T` is the type for parameters.
/// `R` is the type for the return value.
#[derive(Serialize, Deserialize)]
pub struct Action<T, R> {
    /// URI reference used to trigger the action.
    #[serde(rename = "target")]
    pub target: ActionTarget,
    // TODO: we can retrieve constraints on attributes here.
    /// Establishes a dependency on the `T` (parameters) type.
    #[serde(skip)]
    _marker: PhantomData<T>,
    /// Establishes a dependency on the `R` (return value) type.
    #[serde(skip)]
    _marker_retval: PhantomData<R>,
}

impl<T, R> Debug for Action<T, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_struct("Action")
            .field("target", &self.target)
            .finish()
    }
}

/// Action error trait. Needed in generated code when an action function
/// is called for an action that wasn't specified by the server.
pub trait ActionError {
    /// Create an error when the action is not supported.
    fn not_supported() -> Self;
}

impl<T: Send + Sync + Serialize, R: Send + Sync + Sized + for<'de> Deserialize<'de>> Action<T, R> {
    /// Run specific action with parameters passed as argument.
    ///
    /// URI-reference resolution and outbound request policy are handled by
    /// [`Bmc::action`].
    ///
    /// # Errors
    ///
    /// Returns an error if the [`Bmc`] implementation rejects the action
    /// request or if the Redfish service returns an error.
    pub async fn run<B: Bmc>(
        &self,
        bmc: &B,
        params: &T,
    ) -> Result<ModificationResponse<R>, B::Error> {
        bmc.action::<T, R>(self, params).await
    }
}

#[cfg(test)]
mod tests {
    use super::Action;
    use super::ActionTarget;
    use std::marker::PhantomData;

    struct NotDebug;

    #[test]
    fn debug_does_not_require_parameter_or_result_debug() {
        let action: Action<NotDebug, NotDebug> = Action {
            target: ActionTarget::new("/redfish/v1/Actions/Test".into()),
            _marker: PhantomData,
            _marker_retval: PhantomData,
        };

        assert_eq!(
            format!("{action:?}"),
            "Action { target: ActionTarget(\"/redfish/v1/Actions/Test\") }"
        );
    }

    #[test]
    fn serialization_includes_only_the_action_target() {
        let action: Action<NotDebug, NotDebug> = Action {
            target: ActionTarget::new("/redfish/v1/Actions/Test".into()),
            _marker: PhantomData,
            _marker_retval: PhantomData,
        };

        let value = serde_json::to_value(action).expect("action must serialize");

        assert_eq!(
            value,
            serde_json::json!({"target": "/redfish/v1/Actions/Test"})
        );
    }
}
