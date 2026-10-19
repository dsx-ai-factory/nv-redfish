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

use proc_macro2::Ident;
use proc_macro2::Span;

/// Build a shared schema-derived Rust identifier, escaping Rust keywords.
#[must_use]
pub fn escaped(name: &str) -> Ident {
    if matches!(name, "crate" | "self" | "super" | "Self") {
        return Ident::new(&format!("{name}_"), Span::call_site());
    }

    syn::parse_str::<Ident>(name).unwrap_or_else(|_| {
        if name.starts_with(|c: char| c.is_ascii_alphabetic()) {
            Ident::new_raw(name, Span::call_site())
        } else {
            Ident::new(&format!("_{name}"), Span::call_site())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::escaped;

    #[test]
    fn plain_names_pass_through() {
        assert_eq!(escaped("reset").to_string(), "reset");
        assert_eq!(
            escaped("protocol_features_supported").to_string(),
            "protocol_features_supported"
        );
    }

    #[test]
    fn keywords_get_raw_form() {
        assert_eq!(escaped("type").to_string(), "r#type");
        assert_eq!(escaped("override").to_string(), "r#override");
        assert_eq!(escaped("match").to_string(), "r#match");
    }

    #[test]
    fn path_keywords_get_suffix() {
        assert_eq!(escaped("crate").to_string(), "crate_");
        assert_eq!(escaped("self").to_string(), "self_");
        assert_eq!(escaped("super").to_string(), "super_");
        assert_eq!(escaped("Self").to_string(), "Self_");
    }

    #[test]
    fn non_identifier_names_do_not_panic() {
        // `_`, empty, and digit-leading munges are not valid raw
        // identifiers; a leading underscore rescues them instead of
        // panicking in `Ident::new_raw`.
        assert_eq!(escaped("_").to_string(), "__");
        assert_eq!(escaped("").to_string(), "_");
        assert_eq!(escaped("128bit").to_string(), "_128bit");
    }
}
