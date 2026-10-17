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

//! Shared generation of Rust documentation from schema annotations.

use crate::compiler::OData;
use crate::redfish::Deprecation;
use proc_macro2::Delimiter;
use proc_macro2::Group;
use proc_macro2::Ident;
use proc_macro2::Literal;
use proc_macro2::Punct;
use proc_macro2::Spacing;
use proc_macro2::Span;
use proc_macro2::TokenStream;
use proc_macro2::TokenTree;
use std::fmt::Display;

/// Generate Rust documentation from `OData` descriptions.
#[must_use]
pub fn format_and_generate(name: impl Display, odata: &OData<'_>) -> TokenStream {
    format(name, odata)
        .map(|lines| generate(&lines))
        .unwrap_or_default()
}

/// Generate Rust documentation with Redfish deprecation metadata.
#[must_use]
pub fn format_and_generate_with_deprecation(
    name: impl Display,
    odata: &OData<'_>,
    deprecation: Option<Deprecation<'_>>,
) -> TokenStream {
    let mut lines = format(name, odata).unwrap_or_default();
    if let Some(deprecation) = deprecation {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(split_by_lines(&deprecation_text(deprecation)));
    }
    generate(&lines)
}

/// Format long and short descriptions to multiple lines.
#[must_use]
pub fn format(name: impl Display, odata: &OData<'_>) -> Option<Vec<String>> {
    let maybe_descr = odata.description.as_ref().map(ToString::to_string);
    let maybe_long_descr = odata.long_description.as_ref().map(ToString::to_string);
    match (maybe_descr, maybe_long_descr) {
        (None, None) => None,
        (Some(d), None) => Some(vec![format!(" {d}")]),
        (None, Some(ld)) => {
            let mut result = vec![format!(" {}", name), String::new()];
            result.extend(split_by_lines(&ld));
            Some(result)
        }
        (Some(d), Some(ld)) => {
            let mut result = split_by_lines(&d);
            result.push(String::new());
            result.extend(split_by_lines(&ld));
            Some(result)
        }
    }
}

fn deprecation_text(deprecation: Deprecation<'_>) -> String {
    let heading = deprecation.version.map_or_else(
        || "Deprecated in the Redfish schema".to_owned(),
        |version| format!("Deprecated in the Redfish schema since {version}"),
    );
    deprecation.description.map_or_else(
        || format!("{heading}."),
        |description| format!("{heading}: {description}"),
    )
}

/// Generate muliple lines in doc strings in `TokenStream`.
#[must_use]
pub fn generate(lines: &[impl ToString]) -> TokenStream {
    fn ensure_leading_whitespace(line: String) -> String {
        if line.is_empty() || line.starts_with(char::is_whitespace) {
            line
        } else {
            format!(" {line}")
        }
    }

    let mut ts = TokenStream::new();
    for l in lines {
        let line = ensure_leading_whitespace(l.to_string());
        let mut attr_inner = TokenStream::new();
        attr_inner.extend([
            TokenTree::Ident(Ident::new("doc", Span::call_site())),
            TokenTree::Punct(Punct::new('=', Spacing::Alone)),
            TokenTree::Literal(Literal::string(&line)),
        ]);
        ts.extend([
            TokenTree::Punct(Punct::new('#', Spacing::Alone)),
            TokenTree::Group(Group::new(Delimiter::Bracket, attr_inner)),
        ]);
    }
    ts
}

fn split_by_lines(s: &str) -> Vec<String> {
    s.split(' ')
        .fold(
            (Vec::<Vec<&str>>::new(), 0),
            |(mut lines, last_len), word| {
                if let Some(last) = lines.last_mut() {
                    if last_len + word.len() < 100 {
                        last.push(word);
                        return (lines, last_len + word.len() + 1);
                    }
                }
                lines.push(vec![word]);
                (lines, word.len() + 1)
            },
        )
        .0
        .into_iter()
        .map(|words| " ".to_owned() + &words.join(" "))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::deprecation_text;
    use crate::redfish::Deprecation;

    #[test]
    fn formats_redfish_deprecation_as_documentation() {
        assert_eq!(
            deprecation_text(Deprecation {
                version: Some("v1_3_0"),
                description: Some("Use Other for compatibility."),
            }),
            "Deprecated in the Redfish schema since v1_3_0: Use Other for compatibility."
        );
    }
}
