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

use nv_redfish_csdl_compiler::compiler::Config as CompilerConfig;
use nv_redfish_csdl_compiler::compiler::EntityTypeFilter;
use nv_redfish_csdl_compiler::compiler::SchemaBundle;
use nv_redfish_csdl_compiler::edmx::attribute_values::Error as AttributeValuesError;
use nv_redfish_csdl_compiler::edmx::Edmx;
use nv_redfish_csdl_compiler::edmx::ValidateError;
use nv_redfish_csdl_compiler::generator::rust::Config as GeneratorConfig;
use nv_redfish_csdl_compiler::generator::rust::RustGenerator;
use nv_redfish_csdl_compiler::optimizer::optimize;
use nv_redfish_csdl_compiler::optimizer::Config as OptimizerConfig;
use std::io::Error as IoError;
use std::io::Read;

#[allow(dead_code)]
#[derive(Debug)]
enum Error {
    ParameterNeeded,
    Io(String, IoError),
    Edmx(String, ValidateError),
    Compile(String),
    WrongRootService(AttributeValuesError),
    Generate(String),
    ParseGenerated(syn::Error),
}

fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        println!("Usage:");
        println!(" {} <root service> <redfish-csdl-file> ...", args[0]);
        return Err(Error::ParameterNeeded);
    }

    let root_service = args[1].parse().map_err(Error::WrongRootService)?;

    let root_docs = args
        .iter()
        .skip(2)
        .map(|fname| {
            let mut file =
                std::fs::File::open(fname).map_err(|err| Error::Io(fname.clone(), err))?;

            let mut content = String::new();
            file.read_to_string(&mut content)
                .map_err(|err| Error::Io(fname.clone(), err))?;

            Edmx::parse(&content).map_err(|e| Error::Edmx(fname.clone(), e))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let schema_bundle = SchemaBundle::new(root_docs, Vec::new());

    let compiled = schema_bundle
        .compile(
            &[root_service],
            &EntityTypeFilter::new_restrictive(vec![]),
            CompilerConfig::default(),
        )
        .inspect_err(|e| println!("{e}"))
        .map_err(|_| Error::Compile("compilation error".into()))?;
    let compiled = optimize(compiled, &OptimizerConfig::default());
    let generator = RustGenerator::new(compiled, GeneratorConfig::default())
        .inspect_err(|e| println!("{e}"))
        .map_err(|_| Error::Generate("generation error".into()))?;

    let result = generator.generate().to_string();
    // println!("{result}");

    let syntax_tree = syn::parse_file(&result).map_err(Error::ParseGenerated)?;
    println!("{}", prettyplease::unparse(&syntax_tree));

    Ok(())
}
