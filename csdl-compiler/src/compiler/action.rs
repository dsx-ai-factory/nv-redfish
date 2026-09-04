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

use crate::compiler::ensure_type;
use crate::compiler::is_simple_type;
use crate::compiler::Compiled;
use crate::compiler::Context;
use crate::compiler::EntityType;
use crate::compiler::Error;
use crate::compiler::MapType;
use crate::compiler::MustHaveId;
use crate::compiler::Namespace;
use crate::compiler::OData;
use crate::compiler::Parameter;
use crate::compiler::ParameterType;
use crate::compiler::QualifiedName;
use crate::compiler::Stack;
use crate::compiler::TypeInfo;
use crate::edmx::Action as EdmxAction;
use crate::edmx::ActionName;
use crate::edmx::ParameterName;
use crate::redfish::annotations::RedfishAnnotations as _;
use crate::IsNullable;
use crate::IsRequired;
use crate::OneOrCollection;

/// Compiled action.
#[derive(Debug)]
pub struct Action<'a> {
    /// Root namespace of the schema that defines the action.
    pub defining_namespace: Namespace<'a>,
    /// Bound type.
    pub binding: QualifiedName<'a>,
    /// Bound parameter name.
    pub binding_name: &'a ParameterName,
    /// Name of the parameter.
    pub name: &'a ActionName,
    /// Type of the return value.
    pub return_type: Option<OneOrCollection<QualifiedName<'a>>>,
    /// Type of the parameter. Note we reuse `PropertyType`, it
    /// maybe not exact and may be change in future.
    pub parameters: Vec<Parameter<'a>>,
    /// `OData` annotations of the action.
    pub odata: OData<'a>,
}

impl<'a> MapType<'a> for Action<'a> {
    fn map_type<F>(self, f: F) -> Self
    where
        F: Fn(QualifiedName<'a>) -> QualifiedName<'a>,
    {
        Self {
            name: self.name,
            defining_namespace: self.defining_namespace,
            binding: f(self.binding),
            binding_name: self.binding_name,
            return_type: self.return_type.map(|rt| rt.map(&f)),
            parameters: self
                .parameters
                .into_iter()
                .map(|p| p.map_type(&f))
                .collect(),
            odata: self.odata,
        }
    }
}

pub(crate) fn compile_action<'a>(
    action: &'a EdmxAction,
    defining_namespace: Namespace<'a>,
    ctx: &Context<'a>,
    stack: &Stack<'a, '_>,
) -> Result<Compiled<'a>, Error<'a>> {
    if !action.is_bound.into_inner() {
        return Err(Error::NotBoundAction);
    }
    let mut iter = action.parameters.iter();
    let binding_param = iter.next().ok_or(Error::NoBindingParameterForAction)?;
    let binding = binding_param.ptype.qualified_type_name().into();
    let binding_name = &binding_param.name;
    // If the action is bound to a type we haven't compiled, ignore it;
    // there is no node to attach the action to. Note: In generic CSDL
    // this might be unexpected, but in Redfish the binding targets a
    // ComplexType (Actions).
    if stack.complex_type_info(binding).is_none() {
        return Ok(Compiled::default());
    }
    let stack = stack.new_frame();
    // Compile ReturnType if present.
    let (compiled_rt, return_type) = action
        .return_type
        .as_ref()
        .map_or_else(
            || Ok((Compiled::default(), None)),
            |rt| {
                ensure_type(rt.rtype.qualified_type_name().into(), ctx, &stack)
                    .map(|(compiled, _)| (compiled, Some(rt.rtype.as_ref().map(Into::into))))
            },
        )
        .map_err(Box::new)
        .map_err(Error::ActionReturnType)?;
    let stack = stack.merge(compiled_rt);
    // Compile parameters except the first binding parameter.
    let (stack, parameters) = iter.try_fold((stack, Vec::new()), |(cstack, mut params), p| {
        // Sometimes parameters refer to entity types. Unlike properties
        // (which point to complex/simple types) and navigation properties
        // (which point to entity types), actions may take entities. Example:
        // AddResourceBlock in the ComputerSystem schema.
        let qtype_name = p.ptype.qualified_type_name().into();
        let (compiled, ptype) = if is_simple_type(qtype_name) {
            Ok((
                Compiled::default(),
                ParameterType::Type(
                    p.ptype
                        .as_ref()
                        .map(|v| (TypeInfo::simple_type(), v.into())),
                ),
            ))
        } else if ctx.schema_index.find_type(qtype_name).is_some() {
            ensure_type(p.ptype.qualified_type_name().into(), ctx, &cstack).map(
                |(compiled, class)| {
                    (
                        compiled,
                        ParameterType::Type(p.ptype.as_ref().map(|v| (class, v.into()))),
                    )
                },
            )
        } else {
            EntityType::ensure(qtype_name, ctx, &cstack).map(|compiled| {
                (
                    compiled,
                    ParameterType::Entity(p.ptype.as_ref().map(Into::into)),
                )
            })
        }
        .map_err(Box::new)
        .map_err(|e| Error::ActionParameter(&p.name, e))?;
        let nullable = p.nullable.unwrap_or(IsNullable::new(false));
        // Redfish Specification 11.1.5.3 uses an explicit
        // `Nullable="false"` on an action parameter to mark it as required
        // in the request body. Preserve the generic `Redfish.Required`
        // annotation as an additional way to express the same constraint.
        let required = IsRequired::new(
            p.is_required().into_inner()
                || p.nullable.is_some_and(|nullable| !nullable.into_inner()),
        );
        params.push(Parameter {
            name: &p.name,
            ptype,
            nullable,
            required,
            odata: OData::new(MustHaveId::new(false), p),
        });
        Ok((cstack.merge(compiled), params))
    })?;
    Ok(stack
        .merge(Compiled::new_action(Action {
            defining_namespace: defining_namespace.root(),
            binding,
            binding_name,
            name: &action.name,
            return_type,
            parameters,
            odata: OData::new(MustHaveId::new(false), action),
        }))
        .done())
}
