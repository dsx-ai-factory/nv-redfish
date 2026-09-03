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

//! Features manifest
//!
//! Defines a TOML format that selects which CSDL/EDMX files and
//! entity-type patterns to compile. Intended for build scripts to
//! tailor generated code per product or vendor.

use std::collections::HashSet;
use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::fs::File;
use std::io::Error as IoError;
use std::io::Read as _;
use std::path::PathBuf;

use crate::compiler::ActionFilterPattern;
use crate::compiler::EntityTypeFilterPattern;
use crate::compiler::PropertyPattern;

use serde::Deserialize;
use toml::de::Error as TomlError;

/// Root manifest describing standard and OEM feature sets.
#[derive(Deserialize, Debug)]
pub struct FeaturesManifest {
    pub features: Vec<Feature>,
    #[serde(rename = "oem-features")]
    pub oem_features: Vec<OemFeature>,
}

#[derive(Default)]
pub struct Collected<'a> {
    pub csdl_files: Vec<&'a String>,
    pub swordfish_csdl_files: Vec<&'a String>,
    pub patterns: Vec<&'a EntityTypeFilterPattern>,
    pub root_patterns: Vec<&'a EntityTypeFilterPattern>,
    pub rigid_array_patterns: Vec<&'a PropertyPattern>,
}

/// OEM CSDLs and patterns collected for selected vendor features.
#[derive(Default)]
pub struct CollectedOem<'a> {
    pub root_csdls: Vec<&'a String>,
    pub resolve_csdls: Vec<&'a String>,
    pub swordfish_resolve_csdls: Vec<&'a String>,
    pub patterns: Vec<&'a EntityTypeFilterPattern>,
    pub action_patterns: Vec<&'a ActionFilterPattern>,
}

impl FeaturesManifest {
    /// Read a features manifest from a TOML file.
    ///
    /// # Errors
    ///
    /// - `Error::Io` if failed to read file
    /// - `Error::Toml` if failed to parse content as TOML / invalid features manifest.
    pub fn read(fname: &PathBuf) -> Result<Self, Error> {
        let mut file = File::open(fname).map_err(Error::Io)?;
        let mut content = String::new();
        file.read_to_string(&mut content).map_err(Error::Io)?;
        toml::from_str(&content).map_err(Error::Toml)
    }

    /// All standard feature names defined in the manifest.
    #[must_use]
    pub fn all_features(&self) -> Vec<&String> {
        self.features.iter().map(|f| &f.name).collect()
    }

    /// Collect standard CSDLs and patterns for selected features.
    #[must_use]
    pub fn collect<'a>(&'a self, features: &[&String]) -> Collected<'a> {
        self.features
            .iter()
            .fold(Collected::default(), |mut acc, f| {
                if features.contains(&&f.name) {
                    acc.csdl_files.extend(f.csdl_files.iter());
                    acc.swordfish_csdl_files
                        .extend(f.swordfish_csdl_files.iter());
                    acc.patterns.extend(f.patterns.iter());
                    acc.root_patterns.extend(f.root_patterns.iter());
                    acc.rigid_array_patterns.extend(f.rigid_arrays.iter());
                }
                acc
            })
    }

    /// Distinct vendors defined by the manifest, in first-appearance order.
    #[must_use]
    pub fn all_vendors(&self) -> Vec<&String> {
        let mut vendors = Vec::new();
        let mut seen = HashSet::new();

        for feature in &self.oem_features {
            // Deduplicate seen vendors.
            if seen.insert(&feature.vendor) {
                vendors.push(&feature.vendor);
            }
        }

        vendors
    }

    /// All vendor-specific feature names for a vendor.
    #[must_use]
    pub fn all_vendor_features(&self, vendor: &String) -> Vec<&String> {
        self.oem_features
            .iter()
            .filter_map(|f| {
                if f.vendor == *vendor {
                    Some(&f.name)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Collect OEM root/resolve CSDLs and patterns for selected features.
    #[must_use]
    pub fn collect_vendor_features<'a>(
        &'a self,
        vendor: &String,
        features: &[&String],
    ) -> CollectedOem<'a> {
        self.oem_features
            .iter()
            .fold(CollectedOem::default(), |mut collected, f| {
                if f.vendor == *vendor && features.contains(&&f.name) {
                    collected.root_csdls.extend(f.oem_csdl_files.iter());
                    collected.resolve_csdls.extend(f.csdl_files.iter());
                    collected
                        .swordfish_resolve_csdls
                        .extend(f.swordfish_csdl_files.iter());
                    collected.patterns.extend(f.patterns.iter());
                    collected.action_patterns.extend(f.action_patterns.iter());
                }
                collected
            })
    }
}

/// Standard feature block.
#[derive(Deserialize, Debug)]
pub struct Feature {
    pub name: String,
    pub csdl_files: Vec<String>,
    #[serde(default)]
    pub swordfish_csdl_files: Vec<String>,
    pub patterns: Vec<EntityTypeFilterPattern>,
    #[serde(default)]
    pub root_patterns: Vec<EntityTypeFilterPattern>,
    #[serde(default)]
    pub rigid_arrays: Vec<PropertyPattern>,
}

/// OEM-specific feature.
#[derive(Deserialize, Debug)]
pub struct OemFeature {
    /// Name of the feature.
    pub name: String,
    /// Vendor name.
    pub vendor: String,
    /// CSDL files provided by vendor that need to be compiled for the
    /// feature.
    pub oem_csdl_files: Vec<String>,
    /// CSDL files from standard that provide types for vendor CSDL
    /// files.
    pub csdl_files: Vec<String>,
    /// Swordfish CSDL files that provide types for vendor CSDL files.
    ///
    /// Only list files that have no DMTF Redfish counterpart of the
    /// same name -- both directories ship e.g. `Volume_v1.xml`, and
    /// resolving the same namespace twice is an error.
    #[serde(default)]
    pub swordfish_csdl_files: Vec<String>,
    /// Pattern of entity types that need to be resolved during the
    /// compilation.
    #[serde(default)]
    pub patterns: Vec<EntityTypeFilterPattern>,
    /// Actions to include in the OEM root set.
    #[serde(default)]
    pub action_patterns: Vec<ActionFilterPattern>,
}

/// Errors reading or parsing the manifest.
#[derive(Debug)]
pub enum Error {
    Io(IoError),
    Toml(TomlError),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match self {
            Self::Io(err) => write!(f, "input/output error: {err}"),
            Self::Toml(err) => write!(f, "manifest file format error: {err}"),
        }
    }
}

impl StdError for Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_vendors_returns_each_vendor_once_in_manifest_order() {
        let manifest = FeaturesManifest {
            features: Vec::new(),
            oem_features: [
                ("chassis", "nvidia"),
                ("managers", "nvidia"),
                ("attributes", "dell"),
                ("event-service", "nvidia"),
            ]
            .iter()
            .copied()
            .map(|(name, vendor)| OemFeature {
                name: name.into(),
                vendor: vendor.into(),
                oem_csdl_files: Vec::new(),
                csdl_files: Vec::new(),
                swordfish_csdl_files: Vec::new(),
                patterns: Vec::new(),
                action_patterns: Vec::new(),
            })
            .collect(),
        };

        let vendors = manifest
            .all_vendors()
            .into_iter()
            .map(String::as_str)
            .collect::<Vec<_>>();

        assert_eq!(vendors, ["nvidia", "dell"]);
    }
}
