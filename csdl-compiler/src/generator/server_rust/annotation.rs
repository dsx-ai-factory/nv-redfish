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

//! Protocol annotations emitted alongside CSDL properties.

use proc_macro2::{Ident, Literal, TokenStream};

use crate::edmx::PropertyName;
use crate::generator::common_rust::{doc, ident::escaped, StructFieldName};

#[derive(Clone, Copy)]
pub(super) enum Annotation<'a> {
    ODataId,
    ODataType,
    ODataETag,
    ODataContext,
    Count(&'a PropertyName),
}

impl Annotation<'_> {
    /// Protocol metadata fields added to entity responses: `@odata.id`,
    /// `@odata.type`, `@odata.etag`, and `@odata.context`.
    pub const fn entity_metadata() -> [Self; 4] {
        [
            Self::ODataId,
            Self::ODataType,
            Self::ODataETag,
            Self::ODataContext,
        ]
    }

    pub fn documentation(self) -> TokenStream {
        let description = match self {
            Self::ODataId => "The URI that identifies this resource.",
            Self::ODataType => "The resolved CSDL type, initialized from Self::ODATA_TYPE.",
            Self::ODataETag => "The entity tag used for conditional requests.",
            Self::ODataContext => "The OData metadata context for this resource.",
            Self::Count(_) => "Explicit count annotation; it is not inferred from the array.",
        };
        doc::generate(&[description])
    }

    pub fn name(self) -> Ident {
        match self {
            Self::ODataId => escaped("odata_id"),
            Self::ODataType => escaped("odata_type"),
            Self::ODataETag => escaped("odata_etag"),
            Self::ODataContext => escaped("odata_context"),
            Self::Count(property) => escaped(&format!(
                "{}_count",
                StructFieldName::new_property(property)
            )),
        }
    }

    pub fn rename(self) -> Literal {
        match self {
            Self::ODataId => Literal::string("@odata.id"),
            Self::ODataType => Literal::string("@odata.type"),
            Self::ODataETag => Literal::string("@odata.etag"),
            Self::ODataContext => Literal::string("@odata.context"),
            Self::Count(property) => Literal::string(&format!("{property}@odata.count")),
        }
    }
}
