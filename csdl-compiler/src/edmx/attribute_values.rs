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

//! Types defined in 17 Attribute Values

use crate::edmx::QualifiedTypeName;
use crate::OneOrCollection;
use serde::de::Error as DeError;
use serde::de::Visitor;
use serde::Deserialize;
use serde::Deserializer;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::str::FromStr;

#[derive(Debug)]
pub enum Error {
    InvalidSimpleIdentifier(String),
    InvalidQualifiedIdentifier(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::InvalidSimpleIdentifier(id) => write!(f, "invalid simple identifier {id}"),
            Self::InvalidQualifiedIdentifier(id) => write!(f, "invalid qualified identifier {id}"),
        }
    }
}

/// 17.1 `Namespace`
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Namespace {
    pub ids: Vec<SimpleIdentifier>,
}

impl Namespace {
    #[must_use]
    pub fn is_edm(&self) -> bool {
        self.ids.len() == 1 && self.ids[0].inner() == "Edm"
    }
}

impl FromStr for Namespace {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self {
            ids: s
                .split('.')
                .map(SimpleIdentifier::from_str)
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

impl Display for Namespace {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        let mut iter = self.ids.iter();
        if let Some(v) = iter.next() {
            v.fmt(f)?;
        }
        for v in iter {
            ".".fmt(f)?;
            v.fmt(f)?;
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for Namespace {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct NsVisitor {}
        impl Visitor<'_> for NsVisitor {
            type Value = Namespace;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> FmtResult {
                formatter.write_str("Namespace string")
            }
            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }

        de.deserialize_string(NsVisitor {})
    }
}

/// 17.2 `SimpleIdentifier`
#[derive(Clone, Debug, PartialEq, Eq, Ord, PartialOrd, Hash)]
pub struct SimpleIdentifier(String);

impl SimpleIdentifier {
    #[must_use]
    pub const fn inner(&self) -> &String {
        &self.0
    }
}

impl Display for SimpleIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        self.0.fmt(f)
    }
}

impl AsRef<str> for SimpleIdentifier {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl FromStr for SimpleIdentifier {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();

        // Normative: starts with a letter or underscore, followed by
        // at most 127 letters, underscores or digits.
        //
        // Implementation: we don't check max length.
        chars
            .next()
            .and_then(|first| {
                if first.is_alphabetic() || first == '_' {
                    Some(())
                } else {
                    None
                }
            })
            .ok_or_else(|| Error::InvalidSimpleIdentifier(s.into()))?;

        if chars.any(|c| !c.is_alphanumeric() && c != '_') {
            Err(Error::InvalidSimpleIdentifier(s.into()))
        } else {
            Ok(Self(s.into()))
        }
    }
}

impl<'de> Deserialize<'de> for SimpleIdentifier {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct SiVisitor {}
        impl Visitor<'_> for SiVisitor {
            type Value = SimpleIdentifier;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> FmtResult {
                formatter.write_str("SimpleIdentifier string")
            }
            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }

        de.deserialize_string(SiVisitor {})
    }
}

/// 17.3 `QualifiedName`
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct QualifiedName {
    pub namespace: Namespace,
    pub name: SimpleIdentifier,
}

impl Display for QualifiedName {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}.{}", self.namespace, self.name)
    }
}

impl FromStr for QualifiedName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut ids = s
            .split('.')
            .map(SimpleIdentifier::from_str)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| Error::InvalidQualifiedIdentifier(s.into()))?;
        let name = ids
            .pop()
            .ok_or_else(|| Error::InvalidQualifiedIdentifier(s.into()))?;
        Ok(Self {
            namespace: Namespace { ids },
            name,
        })
    }
}

impl<'de> Deserialize<'de> for QualifiedName {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct QnVisitor {}
        impl Visitor<'_> for QnVisitor {
            type Value = QualifiedName;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> FmtResult {
                formatter.write_str("QualifiedName string")
            }
            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }

        de.deserialize_string(QnVisitor {})
    }
}

/// 17.4 `TypeName`
pub type TypeName = OneOrCollection<QualifiedTypeName>;

impl TypeName {
    #[must_use]
    pub const fn qualified_type_name(&self) -> &QualifiedTypeName {
        self.inner()
    }
}

impl FromStr for TypeName {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        const COLLECTION_PREFIX: &str = "Collection(";
        const COLLECTION_SUFFIX: &str = ")";
        if s.starts_with(COLLECTION_PREFIX) && s.ends_with(COLLECTION_SUFFIX) {
            let qtype = s[COLLECTION_PREFIX.len()..s.len() - COLLECTION_SUFFIX.len()].parse()?;
            Ok(Self::Collection(qtype))
        } else {
            Ok(Self::One(s.parse()?))
        }
    }
}

impl<'de> Deserialize<'de> for TypeName {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct QnVisitor {}
        impl Visitor<'_> for QnVisitor {
            type Value = TypeName;

            fn expecting(&self, formatter: &mut Formatter) -> FmtResult {
                formatter.write_str("property type string")
            }

            fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
                value.parse().map_err(DeError::custom)
            }
        }

        de.deserialize_string(QnVisitor {})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::from_str as json_from_str;

    #[test]
    fn test_namespace_valid() {
        // According to 17.1 Namespace, it's dot-separated SimpleIdentifiers
        let valid_cases = vec![
            "Namespace",
            "My.Namespace",
            "My.Complex.Namespace",
            "Edm", // Special case should work and be identified as EDM
        ];

        for case in valid_cases {
            let ns = Namespace::from_str(case);
            assert!(ns.is_ok(), "Failed to parse valid Namespace: {}", case);

            // Verify the correct number of identifiers are parsed
            let ns = ns.unwrap();
            let expected_count = case.chars().filter(|c| *c == '.').count() + 1;
            assert_eq!(ns.ids.len(), expected_count);
        }
    }

    #[test]
    fn test_namespace_invalid() {
        let invalid_cases = vec![
            "Invalid.123Name", // Invalid SimpleIdentifier
            "Namespace.",      // Trailing dot
            ".Namespace",      // Leading dot
            "Namespace..Name", // Double dot
            "",                // Empty string
        ];

        for case in invalid_cases {
            assert!(
                Namespace::from_str(case).is_err(),
                "Should reject invalid Namespace: {}",
                case
            );
        }
    }

    #[test]
    fn test_namespace_is_edm() {
        let edm = Namespace::from_str("Edm").unwrap();
        assert!(edm.is_edm());

        let not_edm = vec![
            Namespace::from_str("NotEdm").unwrap(),
            Namespace::from_str("Edm.Something").unwrap(),
            Namespace::from_str("Something.Edm").unwrap(),
        ];

        for ns in not_edm {
            assert!(!ns.is_edm());
        }
    }

    #[test]
    fn test_namespace_display() {
        let test_cases = vec![
            ("SingleNamespace", "SingleNamespace"),
            ("My.Namespace", "My.Namespace"),
            ("Complex.Name.Space", "Complex.Name.Space"),
        ];

        for (input, expected) in test_cases {
            let ns = Namespace::from_str(input).unwrap();
            assert_eq!(ns.to_string(), expected);
        }
    }

    #[test]
    fn test_namespace_deserialize() {
        let json = r#""My.Valid.Namespace""#;
        let ns: Namespace = json_from_str(json).expect("Should deserialize valid Namespace");
        assert_eq!(ns.ids.len(), 3);

        let json_invalid = r#""Invalid..Namespace""#;
        let result: Result<Namespace, _> = json_from_str(json_invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_simple_identifier_valid() {
        // According to 17.2 SimpleIdentifier, it must start with a letter or underscore
        // and contains only alphanumeric characters and underscores
        let valid_cases = vec![
            "Name",
            "name",
            "_name",
            "Name123",
            "Name_with_underscores",
            "a", // Single letter is valid
        ];

        for case in valid_cases {
            assert!(
                SimpleIdentifier::from_str(case).is_ok(),
                "Failed to parse valid SimpleIdentifier: {}",
                case
            );
        }
    }

    #[test]
    fn test_simple_identifier_invalid() {
        // Invalid cases: starting with digit, containing special characters
        let invalid_cases = vec![
            "123Name",           // Starts with digit
            "Name-with-hyphens", // Contains hyphens
            "Name.with.dots",    // Contains dots
            "Name with spaces",  // Contains spaces
            "",                  // Empty string
            "$Name",             // Starts with special character
        ];

        for case in invalid_cases {
            assert!(
                SimpleIdentifier::from_str(case).is_err(),
                "Should reject invalid SimpleIdentifier: {}",
                case
            );
        }
    }

    #[test]
    fn test_simple_identifier_display() {
        let id = SimpleIdentifier::from_str("TestId").unwrap();
        assert_eq!(id.to_string(), "TestId");
    }

    #[test]
    fn test_simple_identifier_deserialize() {
        // Test JSON deserialization
        let json = r#""ValidName""#;
        let id: SimpleIdentifier =
            json_from_str(json).expect("Should deserialize valid SimpleIdentifier");
        assert_eq!(id.inner(), "ValidName");

        let json_invalid = r#""Invalid-Name""#;
        let result: Result<SimpleIdentifier, _> = json_from_str(json_invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_qualified_name_valid() {
        // According to 17.3 QualifiedName, it's a Namespace + SimpleIdentifier
        let valid_cases = vec![
            "Namespace.Name",
            "My.Namespace.Name",
            "Complex.Namespace.Structure.Name",
        ];

        for case in valid_cases {
            let qn = QualifiedName::from_str(case);
            assert!(qn.is_ok(), "Failed to parse valid QualifiedName: {}", case);

            // Verify the name is the last part
            let qn = qn.unwrap();
            let parts: Vec<&str> = case.split('.').collect();
            assert_eq!(qn.name.to_string(), *parts.last().unwrap());

            // Verify namespace has correct number of parts
            assert_eq!(qn.namespace.ids.len(), parts.len() - 1);
        }
    }

    #[test]
    fn test_qualified_name_invalid() {
        let invalid_cases = vec![
            "Invalid.123Name",      // Invalid SimpleIdentifier (starts with digit)
            "Name-with-hyphens",    // Invalid SimpleIdentifier (contains hyphen)
            "Namespace.",           // Trailing dot (empty SimpleIdentifier)
            ".Namespace",           // Leading dot (empty SimpleIdentifier)
            "Namespace..Name",      // Double dot (empty SimpleIdentifier)
            "",                     // Empty string
            "Name with spaces",     // Invalid SimpleIdentifier (contains space)
            "Name.with.123invalid", // Invalid SimpleIdentifier in namespace
        ];

        for case in invalid_cases {
            assert!(
                QualifiedName::from_str(case).is_err(),
                "Should reject invalid QualifiedName: {}",
                case
            );
        }
    }

    #[test]
    fn test_qualified_name_deserialize() {
        let json = r#""My.Valid.Namespace.Name""#;
        let qn: QualifiedName =
            json_from_str(json).expect("Should deserialize valid QualifiedName");
        assert_eq!(qn.name.to_string(), "Name");
        assert_eq!(qn.namespace.ids.len(), 3);

        let json_invalid = r#""Invalid..Name""#; // Double dot - invalid
        let result: Result<QualifiedName, _> = json_from_str(json_invalid);
        assert!(result.is_err());
    }

    #[test]
    fn test_type_name_valid() {
        // According to 17.4 TypeName can be a qualified name or Collection(qualified name)
        let valid_simple_cases = vec!["Edm.String", "My.Namespace.Type"];

        let valid_collection_cases =
            vec!["Collection(Edm.String)", "Collection(My.Namespace.Type)"];

        // Test simple type names
        for case in valid_simple_cases {
            let tn = TypeName::from_str(case);
            assert!(tn.is_ok(), "Failed to parse valid TypeName: {}", case);

            assert!(
                matches!(tn.unwrap(), TypeName::One(_)),
                "Simple TypeName parsed as Collection: {}",
                case
            );
        }

        // Test collection type names
        for case in valid_collection_cases {
            let tn = TypeName::from_str(case);
            assert!(
                tn.is_ok(),
                "Failed to parse valid Collection TypeName: {}",
                case
            );

            assert!(
                matches!(tn.unwrap(), TypeName::Collection(_)),
                "Collection TypeName parsed as simple: {}",
                case
            );
        }
    }

    #[test]
    fn test_type_name_invalid() {
        let invalid_cases = vec![
            "Collection()",            // Empty collection
            "Collection(Edm/Invalid)", // Invalid qualified name
            "Collection(Edm.String",   // Missing closing parenthesis
            "CollectionEdm.String)",   // Invalid collection syntax
            "Collection Edm.String",   // Space instead of parenthesis
        ];

        for case in invalid_cases {
            assert!(
                TypeName::from_str(case).is_err(),
                "Should reject invalid TypeName: {}",
                case
            );
        }
    }

    #[test]
    fn test_type_name_deserialize() {
        let simple_json = r#""Edm.String""#;
        let simple: TypeName =
            json_from_str(simple_json).expect("Should deserialize valid TypeName");

        assert!(
            matches!(simple, TypeName::One(_)),
            "Simple TypeName deserialized as Collection"
        );

        let collection_json = r#""Collection(Edm.String)""#;
        let collection: TypeName =
            json_from_str(collection_json).expect("Should deserialize valid Collection TypeName");

        assert!(
            matches!(collection, TypeName::Collection(_)),
            "Collection TypeName deserialized as simple"
        );
    }

    // Test error display implementations
    #[test]
    fn test_error_display() {
        let simple_id_error = Error::InvalidSimpleIdentifier("123invalid".to_string());
        let qualified_id_error =
            Error::InvalidQualifiedIdentifier("invalid..qualified".to_string());

        assert_eq!(
            simple_id_error.to_string(),
            "invalid simple identifier 123invalid"
        );
        assert_eq!(
            qualified_id_error.to_string(),
            "invalid qualified identifier invalid..qualified"
        );
    }
}
