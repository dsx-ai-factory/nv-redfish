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

//! Rust naming, documentation, and inheritance shared by client and server generation.

/// Documentation generated from schema annotations.
pub mod doc;
/// Escaped Rust identifiers.
pub mod ident;
/// Borrowed properties and metadata collected through CSDL inheritance.
pub mod inherited_properties;
/// Rust module names derived from CSDL namespaces.
pub mod mod_name;
/// Rust property and parameter names derived from CSDL identifiers.
pub mod property_name;
/// Rust type names derived from CSDL identifiers.
pub mod type_name;

#[doc(inline)]
pub use inherited_properties::InheritedProperties;
#[doc(inline)]
pub use mod_name::ModName;
#[doc(inline)]
pub use property_name::StructFieldName;
#[doc(inline)]
pub use type_name::TypeName;
