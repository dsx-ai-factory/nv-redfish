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

#![doc = include_str!("../README.md")]
#![deny(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    clippy::suspicious,
    clippy::complexity,
    clippy::perf
)]
#![deny(
    clippy::absolute_paths,
    clippy::todo,
    clippy::unimplemented,
    clippy::tests_outside_test_module,
    clippy::panic,
    clippy::unwrap_used,
    clippy::unwrap_in_result,
    clippy::unused_trait_names,
    clippy::print_stdout,
    clippy::print_stderr
)]

//#![deny(missing_docs)]

/// High-level compiler commands.
pub mod commands;
/// Redfish schema compiler.
pub mod compiler;
/// Entity Data Model XML definitions.
pub mod edmx;
/// Compiler errors.
pub mod error;
/// Features manifest.
pub mod features_manifest;
/// Redfish code generator.
pub mod generator;
/// OData-related utilities.
pub mod odata;
/// Type or a collection of a type.
pub mod one_or_collection;
/// Optimizer for compiled data structures.
pub mod optimizer;
/// Schema queries for projection compilers.
pub mod query;
/// Redfish-specific utilities.
pub mod redfish;

use tagged_types::TaggedType;

#[doc(inline)]
pub use error::Error;
#[doc(inline)]
pub use one_or_collection::OneOrCollection;

/// Whether an attribute is nullable.
pub type IsNullable = TaggedType<bool, IsNullableTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Copy, Clone)]
#[transparent(Debug, Deserialize)]
#[capability(inner_access)]
pub enum IsNullableTag {}

/// Whether an attribute is required.
pub type IsRequired = TaggedType<bool, IsRequiredTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug)]
#[capability(inner_access)]
pub enum IsRequiredTag {}

/// Whether an attribute is required when an object is created.
pub type IsRequiredOnCreate = TaggedType<bool, IsRequiredOnCreateTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug)]
#[capability(inner_access)]
pub enum IsRequiredOnCreateTag {}

/// Whether an attribute appears only in excerpt copies of resource.
pub type IsExcerptCopyOnly = TaggedType<bool, IsExcerptCopyOnlyTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug)]
#[capability(inner_access)]
pub enum IsExcerptCopyOnlyTag {}

/// Whether a type is abstract.
pub type IsAbstract = TaggedType<bool, IsAbstractTag>;
#[doc(hidden)]
#[derive(tagged_types::Tag)]
#[implement(Clone, Copy)]
#[transparent(Display, Debug, Deserialize)]
#[capability(inner_access)]
pub enum IsAbstractTag {}

#[cfg(test)]
mod test {
    use super::edmx::attribute_values::SimpleIdentifier;
    use super::edmx::Edmx;
    use crate::Error;
    use std::fs;
    use std::path::Path;

    fn crate_root() -> &'static Path {
        Path::new(env!("CARGO_MANIFEST_DIR"))
    }

    #[test]
    fn test_edmx_element() -> Result<(), Error> {
        let data = r#"
           <edmx:Edmx Version="4.0">
             <edmx:Reference Uri="http://example.com/1.xml"></edmx:Reference>
             <edmx:Reference Uri="http://example.com/2.xml"></edmx:Reference>
             <edmx:DataServices></edmx:DataServices>
           </edmx:Edmx>"#;
        let edmx: Edmx = Edmx::parse(&data).map_err(|err| Error::Edmx("local".into(), err))?;
        assert!(edmx.data_services.schemas.is_empty());
        Ok(())
    }

    #[test]
    fn test_trivial_data() -> Result<(), Error> {
        let data = r#"
           <edmx:Edmx Version="4.0">
             <edmx:DataServices>
               <Schema Namespace="Org.OData.Core.V1" Alias="Core">
                  <Term Name="Computed" Type="Core.Tag" DefaultValue="true" AppliesTo="Property">
                    <Annotation Term="Core.Description" String="A value for this property is generated on both insert and update"/>
                  </Term>
               </Schema>
             </edmx:DataServices>
           </edmx:Edmx>"#;
        let computed: SimpleIdentifier = "Computed".parse().unwrap();
        let edmx: Edmx = Edmx::parse(&data).map_err(|err| Error::Edmx("local".into(), err))?;
        assert_eq!(edmx.data_services.schemas.len(), 1);
        assert_eq!(edmx.data_services.schemas[0].terms.len(), 1);
        assert!(edmx.data_services.schemas[0].terms.get(&computed).is_some());
        if let Some(term) = &edmx.data_services.schemas[0].terms.get(&computed) {
            assert_eq!(term.ttype.as_ref().unwrap(), &"Core.Tag".parse().unwrap());
            assert_eq!(term.default_value.as_ref().unwrap(), "true");
        }
        Ok(())
    }

    #[ignore]
    #[test]
    fn test_read_odata() -> Result<(), Error> {
        let fname = crate_root().join("test-data/edmx/odata-4.0.xml");
        let fname_string = fname.display().to_string();
        let data = fs::read_to_string(fname).map_err(|err| Error::Io(fname_string.clone(), err))?;
        let _edmx: Edmx = Edmx::parse(&data).map_err(|err| Error::Edmx(fname_string, err))?;
        Ok(())
    }

    #[ignore]
    #[test]
    fn test_read_redfish() -> Result<(), Error> {
        let fname = crate_root().join("test-data/redfish-schema/CoolantConnector_v1.xml");
        let fname_string = fname.display().to_string();
        let data = fs::read_to_string(fname).map_err(|err| Error::Io(fname_string.clone(), err))?;
        let edmx: Edmx = Edmx::parse(&data).map_err(|err| Error::Edmx(fname_string, err))?;
        assert_eq!(edmx.data_services.schemas.len(), 6);
        assert_eq!(edmx.data_services.schemas.get(1).unwrap().types.len(), 4);
        assert_eq!(
            edmx.data_services
                .schemas
                .get(1)
                .unwrap()
                .entity_types
                .len(),
            1
        );
        assert_eq!(
            edmx.data_services.schemas.get(1).unwrap().annotations.len(),
            2
        );
        Ok(())
    }
}
