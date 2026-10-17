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

//! Client and server code generation from the compiled CSDL model.
//!
//! The conventions below apply when extending either generator. Keep their
//! concepts and naming aligned without forcing different output requirements
//! into a shared implementation.
//!
//! # Prepare schema data, then emit Rust
//!
//! Preparation collects borrowed schema declarations, resolves inheritance and
//! dependencies, and selects model shapes. Keep identifiers, property types,
//! annotations, descriptions, and cardinality as typed data at this stage.
//! Do not pre-render them into strings, documentation attributes, or token streams.
//!
//! Use `ModDef`, `StructDef`, `EnumDef`, and `TypeDef` for their respective
//! responsibilities. Module definitions organize prepared definitions; they
//! should not need the entire `Compiled` bundle to rediscover those definitions
//! during emission. Preserve schema names in module trees until Rust names are
//! needed for output, module merging, or collision checks.
//! Optimizations must preserve the wire identity of selected roots: identical
//! fields do not make two schema versions interchangeable for `@odata.type`.
//! Excerpt copies are separate model shapes, not resource references or full
//! entities. Select fields by excerpt key, keep excerpt-only fields out of full
//! resources, and do not add automatic resource metadata to excerpt models.
//!
//! Rendering may construct and reuse tokens locally. For example, the server's
//! `Property` describes a borrowed source declaration or protocol annotation,
//! while `PropertyDef` prepares field types, serde attributes, documentation,
//! and setters only inside struct emission.
//!
//! # Types and names express meaning
//!
//! Reuse `QualifiedName`, `ModName`, `TypeName`, and property-name wrappers rather
//! than storing their string representations. Preserve borrowed, `Copy` values
//! where possible. Keep protocol annotations distinct from CSDL property names.
//! Qualify standard-library traits in emitted code: schema types may themselves
//! be named `Into`, `IntoIterator`, or `Default`.
//! Use `Path`/`PathBuf` for filesystem paths and `IsRequired`/`IsNullable` for
//! property flags. Avoid boolean arguments that select unrelated behavior; name
//! the operation directly or derive the choice from existing schema data.
//!
//! Name fields according to the value actually selected, including inheritance
//! semantics: `nearest_odata_with_description` is not an arbitrary `odata` value.
//! Generate property documentation from that property's own annotations; do not
//! copy the description of its referenced type onto the property. Type
//! documentation may use the nearest description in its inheritance hierarchy.
//!
//! # Make transformations explicit
//!
//! Collection helpers return their result or an iterator instead of appending
//! to a caller-owned `&mut Vec`. Prefer `map`, `flat_map`, `chain`, and `collect`
//! for transformations, and `try_fold` for coordinated fallible accumulation.
//! Keep mutation local to the accumulation step or a small helper, including a
//! nested function when that makes its purpose explicit. Prefer success-first
//! expressions over negated conditions and early returns in transformation steps.
//!
//! # Preserve diagnostics and test owned behavior
//!
//! Error payloads retain typed identifiers, paths, and underlying errors.
//! `Display` supplies context; `source()` supplies the wrapped cause, without
//! printing that cause again in `Display`. A borrowed compiler error needs an
//! owned representation to cross the file-loading boundary; the existing
//! `Compile(String)` conversion is a deferred limitation, not a pattern to copy.
//!
//! Put tests with the component whose behavior they exercise: generator tests
//! for emitted code and diagnostics, and core unit tests for core primitives.
//! Keep examples as examples, without test-only schemas or test infrastructure.
//! Test observable behavior rather than the borrow checker, standard containers,
//! or serde itself. Checking that generated Rust compiles can validate codegen;
//! merely compiling ordinary handwritten Rust is not a runtime test objective.

/// Rust client read models and request types.
pub mod client_rust;

/// Shared Rust naming and documentation generation.
pub mod common_rust;

/// Serializable Rust server models.
pub mod server_rust;

/// manipulate strings to produce patterns suitable for different languages (e.g, `CamelCase` to `snake_case` conversion)
pub mod casemungler;
