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

//! Repairing documents before they are deserialized.
//!
//! A [`Repair`] rewrites a document a device sent so it matches the schema
//! type it is read as. A [`Bmc`](crate::Bmc) applies it through
//! [`Bmc::get_repaired`](crate::Bmc::get_repaired) and its siblings: a
//! transport that deserializes documents itself repairs in the same pass,
//! and any other reads the document as [`Raw`] and hands it to
//! [`decode_raw`].

use std::error::Error as StdError;
use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::Value;

use crate::ActionError;
use crate::BmcError;
use crate::BmcErrorClass;
use crate::Raw;

/// A rewrite of documents before they are deserialized.
pub trait Repair: Send + Sync {
    /// Rewrites `document` in place.
    fn repair(&self, document: &mut Value);

    /// Identifies the rewrite: two repairs with the same generation rewrite
    /// every document the same way, and `None` leaves every document as it
    /// came. A transport that caches results keys them by it, so a result
    /// repaired under another generation is not served.
    fn generation(&self) -> Option<Generation>;
}

/// The identity of a [`Repair`]'s rewrite, as a cache sees it.
///
/// A generation is either [`unique`](Self::unique) or a
/// [`variant`](Self::variant) of a unique one, so two independent repairs
/// cannot present the same generation to a cache.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Generation {
    id: u64,
    variant: u64,
}

impl Generation {
    /// A generation unequal to every other one in the process.
    ///
    /// Ids come from one process-wide 64-bit counter: at a million calls a
    /// second it wraps after more than half a million years.
    #[must_use]
    pub fn unique() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            variant: 0,
        }
    }

    /// The variant `variant` of this generation. Variants of one generation
    /// differ from each other, and from the variants of every other one;
    /// variant `0` is the generation itself.
    #[must_use]
    pub const fn variant(self, variant: u64) -> Self {
        Self {
            id: self.id,
            variant,
        }
    }
}

/// The document as it came: the repair of a read that asks for none.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRepair;

impl Repair for NoRepair {
    fn repair(&self, _: &mut Value) {}

    fn generation(&self) -> Option<Generation> {
        None
    }
}

/// A document that did not deserialize, with the path to the property that
/// failed.
pub type DecodeError = serde_path_to_error::Error<serde_json::Error>;

/// What a repaired read can fail with.
#[derive(Debug)]
pub enum RepairError<E> {
    /// The transport failed.
    Transport(E),
    /// The document, repaired, did not deserialize into the type asked for.
    Decode(DecodeError),
}

impl<E: BmcError> RepairError<E> {
    /// A transport's failure, with its own failure to deserialize a
    /// document reported as [`Self::Decode`].
    #[must_use]
    pub fn from_transport(error: E) -> Self {
        error
            .into_decode_error()
            .map_or_else(Self::Transport, Self::Decode)
    }
}

impl<E: fmt::Display> fmt::Display for RepairError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(error) => write!(f, "BMC error: {error}"),
            Self::Decode(error) => write!(f, "document did not deserialize: {error}"),
        }
    }
}

impl<E: StdError + 'static> StdError for RepairError<E> {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Transport(error) => Some(error),
            Self::Decode(error) => Some(error),
        }
    }
}

impl<E: BmcError + 'static> BmcError for RepairError<E> {
    fn error_class(&self) -> BmcErrorClass {
        match self {
            Self::Transport(error) => error.error_class(),
            Self::Decode(_) => BmcErrorClass::ResponseParse,
        }
    }

    fn into_decode_error(self) -> Result<DecodeError, Self> {
        match self {
            Self::Transport(error) => error.into_decode_error().map_err(Self::Transport),
            Self::Decode(error) => Ok(error),
        }
    }
}

impl<E: ActionError> ActionError for RepairError<E> {
    fn not_supported() -> Self {
        Self::Transport(E::not_supported())
    }
}

/// Deserializes `document` into `T`, recording the path to the property
/// that failed.
///
/// # Errors
///
/// `document` does not deserialize into `T`.
pub fn decode<T>(document: Value) -> Result<T, DecodeError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_path_to_error::deserialize(document)
}

/// Repairs a document read as [`Raw`] and deserializes it into `T`: the
/// repaired read of a transport that does not deserialize documents itself.
///
/// # Errors
///
/// The read failed, or the repaired document did not deserialize into `T`.
pub fn decode_raw<T, E>(
    raw: Result<Arc<Raw>, E>,
    repair: &impl Repair,
) -> Result<Arc<T>, RepairError<E>>
where
    T: for<'de> Deserialize<'de>,
{
    let raw = raw.map_err(RepairError::Transport)?;
    // A transport that does not cache hands over the only reference.
    let mut document = Arc::try_unwrap(raw).map_or_else(|raw| raw.value().clone(), Raw::into_value);
    repair.repair(&mut document);
    decode(document).map(Arc::new).map_err(RepairError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generations_never_collide_across_repairs() {
        let first = Generation::unique();
        let second = Generation::unique();
        assert_ne!(first, second);
        assert_eq!(first.variant(0), first);
        assert_ne!(first.variant(1), first);
        assert_ne!(first.variant(1), second.variant(1));
        assert_eq!(first.variant(7), first.variant(7));
    }
}
