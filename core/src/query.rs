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

//! OData query parameter builders for Redfish API requests.
//!
//! This module provides type-safe builders for constructing OData query parameters
//! according to the Redfish specification (DSP0266). These query parameters allow
//! clients to customize API responses through resource expansion and filtering.
//!
//! # Query Parameters
//!
//! ## Expand Query (`$expand`)
//!
//! The [`ExpandQuery`] builder constructs `$expand` parameters to request inline expansion
//! of navigation properties, reducing the number of HTTP requests needed.
//!
//! ```rust
//! use nv_redfish_core::query::ExpandQuery;
//!
//! // Expand current resource with 2 levels
//! let query = ExpandQuery::current().levels(2);
//! assert_eq!(query.to_query_string(), "$expand=.($levels=2)");
//! ```
//!
//! ## Filter Query (`$filter`)
//!
//! The [`FilterQuery`] builder constructs `$filter` parameters to request server-side
//! filtering of collection members or resource properties using OData filter expressions.
//!
//! ```rust
//! use nv_redfish_core::query::FilterQuery;
//!
//! // Filter for resources where Status/Health equals "OK"
//! let query = FilterQuery::eq(&"Status/Health", "OK");
//! assert_eq!(query.to_query_string(), "$filter=Status/Health eq 'OK'");
//!
//! // Complex filter with logical operators
//! let query = FilterQuery::gt(&"Temperature", 50)
//!     .and()
//!     .lt_then(&"Temperature", 80);
//! assert_eq!(query.to_query_string(), "$filter=Temperature gt 50 and Temperature lt 80");
//! ```
//!
//! # Type Safety
//!
//! Both builders use traits to ensure type safety:
//!
//! - [`crate::FilterProperty`]: Types that can be used as filter property paths
//! - [`ToFilterLiteral`]: Types that can be converted to filter literal values
//!
//! Property paths can be:
//! - String literals (`"PropertyName"`)
//! - Generated property accessors from CSDL compilation
//! - Nested paths (`"Parent/Child"`)
//!
//! # References
//!
//! - [Redfish Specification DSP0266](https://redfish.dmtf.org/schemas/DSP0266_1.15.0.html)
//! - [OData Version 4.0 Protocol](http://docs.oasis-open.org/odata/odata/v4.0/os/part2-url-conventions/odata-v4.0-os-part2-url-conventions.html)

/// Builder for Redfish `$expand` query parameters according to DSP0266 specification.
///
/// The `$expand` query parameter allows clients to request that the server expand
/// navigation properties inline instead of returning just references. This is particularly
/// useful for reducing the number of HTTP requests needed to retrieve related data.
///
/// According to the [Redfish specification Table 9](https://redfish.dmtf.org/schemas/DSP0266_1.15.0.html#the-expand-query-parameter),
/// the supported expand options are:
///
/// | Option | Description | Example URL |
/// |--------|-------------|-------------|
/// | `*` | Expand all hyperlinks, including payload annotations | `?$expand=*` |
/// | `.` | Expand hyperlinks not in links property instances | `?$expand=.` |
/// | `~` | Expand hyperlinks in links property instances | `?$expand=~` |
/// | `$levels` | Number of levels to cascade expansion | `?$expand=.($levels=2)` |
///
/// # Examples
///
/// ```rust
/// use nv_redfish_core::query::ExpandQuery;
///
/// // Default: expand current resource one level
/// let default = ExpandQuery::default();
/// assert_eq!(default.to_query_string(), "$expand=.($levels=1)");
///
/// // Expand all hyperlinks
/// let all = ExpandQuery::all();
/// assert_eq!(all.to_query_string(), "$expand=*($levels=1)");
///
/// // Expand with multiple levels
/// let deep = ExpandQuery::current().levels(3);
/// assert_eq!(deep.to_query_string(), "$expand=.($levels=3)");
///
/// // Expand specific navigation property
/// let thermal = ExpandQuery::property("Thermal");
/// assert_eq!(thermal.to_query_string(), "$expand=Thermal($levels=1)");
/// ```
#[derive(Debug, Clone)]
pub struct ExpandQuery {
    /// The expand expression (*, ., ~, or specific navigation properties)
    expand_expression: String,
    /// Number of levels to cascade the expand operation (default is 1)
    levels: Option<u32>,
}

impl Default for ExpandQuery {
    /// Default expand query: $expand=.($levels=1)
    /// Expands all hyperlinks not in any links property instances of the resource
    fn default() -> Self {
        Self {
            expand_expression: ".".to_string(),
            levels: Some(1),
        }
    }
}

impl ExpandQuery {
    /// Create a new expand query with default values.
    ///
    /// This is equivalent to [`ExpandQuery::default()`] and creates a query
    /// that expands the current resource one level deep: `$expand=.($levels=1)`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let query = ExpandQuery::new();
    /// assert_eq!(query.to_query_string(), "$expand=.($levels=1)");
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Expand all hyperlinks excluding links.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let query = ExpandQuery::no_links();
    /// assert_eq!(query.to_query_string(), "$expand=.");
    /// ```
    #[must_use]
    pub fn no_links() -> Self {
        Self {
            expand_expression: ".".to_string(),
            levels: None,
        }
    }

    /// Expand all hyperlinks, including those in payload annotations.
    ///
    /// This expands all hyperlinks found in the resource, including those in payload
    /// annotations such as `@Redfish.Settings`, `@Redfish.ActionInfo`, and
    /// `@Redfish.CollectionCapabilities`.
    ///
    /// Equivalent to: `$expand=*`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let query = ExpandQuery::all();
    /// assert_eq!(query.to_query_string(), "$expand=*($levels=1)");
    ///
    /// // With multiple levels
    /// let deep = ExpandQuery::all().levels(3);
    /// assert_eq!(deep.to_query_string(), "$expand=*($levels=3)");
    /// ```
    #[must_use]
    pub fn all() -> Self {
        Self {
            expand_expression: "*".to_string(),
            levels: Some(1),
        }
    }

    /// Expand all hyperlinks not in any links property instances of the resource.
    ///
    /// This expands hyperlinks found directly in the resource properties, but not
    /// those in dedicated `Links` sections. Includes payload annotations.
    ///
    /// Equivalent to: `$expand=.`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let query = ExpandQuery::current();
    /// assert_eq!(query.to_query_string(), "$expand=.($levels=1)");
    /// ```
    #[must_use]
    pub fn current() -> Self {
        Self {
            expand_expression: ".".to_string(),
            levels: Some(1),
        }
    }

    /// Expand all hyperlinks found in all links property instances of the resource.
    ///
    /// This expands only hyperlinks found in `Links` sections of the resource,
    /// which typically contain references to related resources.
    ///
    /// Equivalent to: `$expand=~`
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let query = ExpandQuery::links();
    /// assert_eq!(query.to_query_string(), "$expand=~($levels=1)");
    /// ```
    #[must_use]
    pub fn links() -> Self {
        Self {
            expand_expression: "~".to_string(),
            levels: Some(1),
        }
    }

    /// Expand a specific navigation property.
    ///
    /// This expands only the specified navigation property, which is useful when you
    /// know exactly which related data you need.
    ///
    /// # Arguments
    ///
    /// * `property` - The name of the navigation property to expand
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let thermal = ExpandQuery::property("Thermal");
    /// assert_eq!(thermal.to_query_string(), "$expand=Thermal($levels=1)");
    ///
    /// let members = ExpandQuery::property("Members");
    /// assert_eq!(members.to_query_string(), "$expand=Members($levels=1)");
    /// ```
    pub fn property<S: Into<String>>(property: S) -> Self {
        Self {
            expand_expression: property.into(),
            levels: Some(1),
        }
    }

    /// Expand multiple specific navigation properties.
    ///
    /// This allows expanding several navigation properties in a single request,
    /// which is more efficient than making separate requests for each property.
    ///
    /// # Arguments
    ///
    /// * `properties` - A slice of navigation property names to expand
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let env = ExpandQuery::properties(&["Thermal", "Power"]);
    /// assert_eq!(env.to_query_string(), "$expand=Thermal,Power($levels=1)");
    ///
    /// let system = ExpandQuery::properties(&["Processors", "Memory", "Storage"]);
    /// assert_eq!(system.to_query_string(), "$expand=Processors,Memory,Storage($levels=1)");
    /// ```
    #[must_use]
    pub fn properties(properties: &[&str]) -> Self {
        Self {
            expand_expression: properties.join(","),
            levels: Some(1),
        }
    }

    /// Set the number of levels to cascade the expand operation.
    ///
    /// The `$levels` parameter controls how deep the expansion goes:
    /// - Level 1: Expand hyperlinks in the current resource
    /// - Level 2: Also expand hyperlinks in the resources expanded at level 1
    /// - And so on...
    ///
    /// # Arguments
    ///
    /// * `levels` - Number of levels to expand (typically 1-6 in practice)
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let shallow = ExpandQuery::current().levels(1);
    /// assert_eq!(shallow.to_query_string(), "$expand=.($levels=1)");
    ///
    /// let deep = ExpandQuery::all().levels(3);
    /// assert_eq!(deep.to_query_string(), "$expand=*($levels=3)");
    /// ```
    #[must_use]
    pub const fn levels(mut self, levels: u32) -> Self {
        self.levels = Some(levels);
        self
    }

    /// Convert to the `OData` query string according to Redfish specification.
    ///
    /// This generates the actual query parameter string that will be appended to
    /// HTTP requests to Redfish services.
    ///
    /// # Returns
    ///
    /// A query string in the format `$expand=expression($levels=n)` or just
    /// `$expand=expression` if no levels are specified.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use nv_redfish_core::query::ExpandQuery;
    ///
    /// let query = ExpandQuery::property("Thermal").levels(2);
    /// assert_eq!(query.to_query_string(), "$expand=Thermal($levels=2)");
    ///
    /// let query = ExpandQuery::all();
    /// assert_eq!(query.to_query_string(), "$expand=*($levels=1)");
    /// ```
    #[must_use]
    #[allow(clippy::option_if_let_else)]
    pub fn to_query_string(&self) -> String {
        match self.levels {
            Some(levels) => format!("$expand={}($levels={})", self.expand_expression, levels),
            None => format!("$expand={}", self.expand_expression),
        }
    }
}

/// Literal value types supported in filter expressions
#[derive(Debug, Clone)]
pub enum FilterLiteral {
    /// String literal value
    String(String),
    /// Floating point number literal value
    Number(f64),
    /// Integer literal value
    Integer(i64),
    /// Boolean literal value
    Boolean(bool),
}

impl FilterLiteral {
    fn to_odata_string(&self) -> String {
        match self {
            Self::String(s) => format!("'{}'", s.replace('\'', "''")),
            Self::Number(n) => n.to_string(),
            Self::Integer(i) => i.to_string(),
            Self::Boolean(b) => b.to_string(),
        }
    }
}

/// Trait for types that can be converted to filter literals
pub trait ToFilterLiteral {
    /// Convert this value to a filter literal
    fn to_filter_literal(self) -> FilterLiteral;
}

impl ToFilterLiteral for &str {
    fn to_filter_literal(self) -> FilterLiteral {
        FilterLiteral::String(self.to_string())
    }
}

impl ToFilterLiteral for String {
    fn to_filter_literal(self) -> FilterLiteral {
        FilterLiteral::String(self)
    }
}

impl ToFilterLiteral for i32 {
    fn to_filter_literal(self) -> FilterLiteral {
        FilterLiteral::Integer(i64::from(self))
    }
}

impl ToFilterLiteral for i64 {
    fn to_filter_literal(self) -> FilterLiteral {
        FilterLiteral::Integer(self)
    }
}

impl ToFilterLiteral for f64 {
    fn to_filter_literal(self) -> FilterLiteral {
        FilterLiteral::Number(self)
    }
}

impl ToFilterLiteral for bool {
    fn to_filter_literal(self) -> FilterLiteral {
        FilterLiteral::Boolean(self)
    }
}

/// Filter expression component
#[derive(Debug, Clone)]
enum FilterExpr {
    Comparison {
        property: String,
        operator: &'static str,
        value: FilterLiteral,
    },
    And(Box<Self>, Box<Self>),
    Or(Box<Self>, Box<Self>),
    Not(Box<Self>),
    Group(Box<Self>),
}

impl FilterExpr {
    fn to_odata_string(&self) -> String {
        match self {
            Self::Comparison {
                property,
                operator,
                value,
            } => {
                format!("{} {} {}", property, operator, value.to_odata_string())
            }
            Self::And(left, right) => {
                format!("{} and {}", left.to_and_operand(), right.to_and_operand())
            }
            Self::Or(left, right) => {
                format!("{} or {}", left.to_odata_string(), right.to_odata_string())
            }
            // DSP0266 gives `not` a higher precedence than comparisons, so
            // the operand always needs parentheses.
            Self::Not(expr) => match expr.as_ref() {
                Self::Group(_) => format!("not {}", expr.to_odata_string()),
                _ => format!("not ({})", expr.to_odata_string()),
            },
            Self::Group(expr) => {
                format!("({})", expr.to_odata_string())
            }
        }
    }

    /// `and` binds tighter than `or`: keep an `or` operand together.
    fn to_and_operand(&self) -> String {
        match self {
            Self::Or(..) => format!("({})", self.to_odata_string()),
            _ => self.to_odata_string(),
        }
    }
}

/// Builder for Redfish `$filter` query parameters according to DSP0266 specification.
///
/// The `$filter` query parameter allows clients to request a subset of collection members
/// based on comparison and logical expressions.
///
/// # Supported Operators
///
/// - Comparison: `eq`, `ne`, `gt`, `ge`, `lt`, `le`
/// - Logical: `and`, `or`, `not`
/// - Grouping: `()`
///
/// # Examples
///
/// ```rust
/// use nv_redfish_core::query::FilterQuery;
///
/// // Simple equality
/// let filter = FilterQuery::eq(&"ProcessorSummary/Count", 2);
/// assert_eq!(filter.to_query_string(), "$filter=ProcessorSummary/Count eq 2");
///
/// // Complex expression with logical operators
/// let filter = FilterQuery::eq(&"ProcessorSummary/Count", 2)
///     .and()
///     .gt_then(&"MemorySummary/TotalSystemMemoryGiB", 64);
/// assert_eq!(
///     filter.to_query_string(),
///     "$filter=ProcessorSummary/Count eq 2 and MemorySummary/TotalSystemMemoryGiB gt 64"
/// );
///
/// // With grouping
/// let filter = FilterQuery::eq(&"Status/State", "Enabled")
///     .and()
///     .eq_then(&"Status/Health", "OK")
///     .group()
///     .or()
///     .eq_then(&"SystemType", "Physical");
/// ```
#[derive(Debug, Clone)]
pub struct FilterQuery {
    expr: Option<FilterExpr>,
    pending_logical_op: Option<LogicalOp>,
}

#[derive(Debug, Clone, Copy)]
enum LogicalOp {
    And,
    Or,
}

impl FilterQuery {
    /// Create a new filter with an equality comparison
    pub fn eq<P: crate::FilterProperty, V: ToFilterLiteral>(property: &P, value: V) -> Self {
        Self {
            expr: Some(FilterExpr::Comparison {
                property: property.property_path().to_string(),
                operator: "eq",
                value: value.to_filter_literal(),
            }),
            pending_logical_op: None,
        }
    }

    /// Create a new filter with a not-equal comparison
    pub fn ne<P: crate::FilterProperty, V: ToFilterLiteral>(property: &P, value: V) -> Self {
        Self {
            expr: Some(FilterExpr::Comparison {
                property: property.property_path().to_string(),
                operator: "ne",
                value: value.to_filter_literal(),
            }),
            pending_logical_op: None,
        }
    }

    /// Create a new filter with a greater-than comparison
    pub fn gt<P: crate::FilterProperty, V: ToFilterLiteral>(property: &P, value: V) -> Self {
        Self {
            expr: Some(FilterExpr::Comparison {
                property: property.property_path().to_string(),
                operator: "gt",
                value: value.to_filter_literal(),
            }),
            pending_logical_op: None,
        }
    }

    /// Create a new filter with a greater-than-or-equal comparison
    pub fn ge<P: crate::FilterProperty, V: ToFilterLiteral>(property: &P, value: V) -> Self {
        Self {
            expr: Some(FilterExpr::Comparison {
                property: property.property_path().to_string(),
                operator: "ge",
                value: value.to_filter_literal(),
            }),
            pending_logical_op: None,
        }
    }

    /// Create a new filter with a less-than comparison
    pub fn lt<P: crate::FilterProperty, V: ToFilterLiteral>(property: &P, value: V) -> Self {
        Self {
            expr: Some(FilterExpr::Comparison {
                property: property.property_path().to_string(),
                operator: "lt",
                value: value.to_filter_literal(),
            }),
            pending_logical_op: None,
        }
    }

    /// Create a new filter with a less-than-or-equal comparison
    pub fn le<P: crate::FilterProperty, V: ToFilterLiteral>(property: &P, value: V) -> Self {
        Self {
            expr: Some(FilterExpr::Comparison {
                property: property.property_path().to_string(),
                operator: "le",
                value: value.to_filter_literal(),
            }),
            pending_logical_op: None,
        }
    }

    /// Add a logical AND operator (must be followed by another comparison)
    #[must_use]
    pub const fn and(mut self) -> Self {
        self.pending_logical_op = Some(LogicalOp::And);
        self
    }

    /// Add a logical OR operator (must be followed by another comparison)
    #[must_use]
    pub const fn or(mut self) -> Self {
        self.pending_logical_op = Some(LogicalOp::Or);
        self
    }

    /// Wrap the current expression in a NOT operator
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn not(mut self) -> Self {
        if let Some(expr) = self.expr.take() {
            self.expr = Some(FilterExpr::Not(Box::new(expr)));
        }
        self
    }

    /// Wrap the current expression in grouping parentheses
    #[must_use]
    pub fn group(mut self) -> Self {
        if let Some(expr) = self.expr.take() {
            self.expr = Some(FilterExpr::Group(Box::new(expr)));
        }
        self
    }

    /// Chain an equality comparison (after .`and()` or .`or()`)
    #[must_use]
    pub fn eq_then<P: crate::FilterProperty, V: ToFilterLiteral>(
        self,
        property: &P,
        value: V,
    ) -> Self {
        let new_expr = FilterExpr::Comparison {
            property: property.property_path().to_string(),
            operator: "eq",
            value: value.to_filter_literal(),
        };
        self.combine_with_pending_op(new_expr)
    }

    /// Chain a not-equal comparison (after .`and()` or .`or()`)
    #[must_use]
    pub fn ne_then<P: crate::FilterProperty, V: ToFilterLiteral>(
        self,
        property: &P,
        value: V,
    ) -> Self {
        let new_expr = FilterExpr::Comparison {
            property: property.property_path().to_string(),
            operator: "ne",
            value: value.to_filter_literal(),
        };
        self.combine_with_pending_op(new_expr)
    }

    /// Chain a greater-than comparison (after .`and()` or .`or()`)
    #[must_use]
    pub fn gt_then<P: crate::FilterProperty, V: ToFilterLiteral>(
        self,
        property: &P,
        value: V,
    ) -> Self {
        let new_expr = FilterExpr::Comparison {
            property: property.property_path().to_string(),
            operator: "gt",
            value: value.to_filter_literal(),
        };
        self.combine_with_pending_op(new_expr)
    }

    /// Chain a greater-than-or-equal comparison (after .`and()` or .`or()`)
    #[must_use]
    pub fn ge_then<P: crate::FilterProperty, V: ToFilterLiteral>(
        self,
        property: &P,
        value: V,
    ) -> Self {
        let new_expr = FilterExpr::Comparison {
            property: property.property_path().to_string(),
            operator: "ge",
            value: value.to_filter_literal(),
        };
        self.combine_with_pending_op(new_expr)
    }

    /// Chain a less-than comparison (after .`and()` or .`or()`)
    #[must_use]
    pub fn lt_then<P: crate::FilterProperty, V: ToFilterLiteral>(
        self,
        property: &P,
        value: V,
    ) -> Self {
        let new_expr = FilterExpr::Comparison {
            property: property.property_path().to_string(),
            operator: "lt",
            value: value.to_filter_literal(),
        };
        self.combine_with_pending_op(new_expr)
    }

    /// Chain a less-than-or-equal comparison (after .`and()` or .`or()`)
    #[must_use]
    pub fn le_then<P: crate::FilterProperty, V: ToFilterLiteral>(
        self,
        property: &P,
        value: V,
    ) -> Self {
        let new_expr = FilterExpr::Comparison {
            property: property.property_path().to_string(),
            operator: "le",
            value: value.to_filter_literal(),
        };
        self.combine_with_pending_op(new_expr)
    }

    fn combine_with_pending_op(mut self, new_expr: FilterExpr) -> Self {
        if let Some(existing) = self.expr.take() {
            self.expr = Some(match self.pending_logical_op.take() {
                Some(LogicalOp::And) => FilterExpr::And(Box::new(existing), Box::new(new_expr)),
                Some(LogicalOp::Or) => FilterExpr::Or(Box::new(existing), Box::new(new_expr)),
                None => new_expr,
            });
        } else {
            self.expr = Some(new_expr);
        }
        self
    }

    /// Convert to the `OData` query string
    #[must_use]
    pub fn to_query_string(&self) -> String {
        self.expr.as_ref().map_or_else(String::new, |expr| {
            format!("$filter={}", expr.to_odata_string())
        })
    }
}

/// Implement `FilterProperty` for `&str`
impl crate::FilterProperty for &str {
    fn property_path(&self) -> &str {
        self
    }
}

/// Implement `FilterProperty` for `String`
impl crate::FilterProperty for String {
    fn property_path(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_expand() {
        let query = ExpandQuery::default();
        assert_eq!(query.to_query_string(), "$expand=.($levels=1)");
    }

    #[test]
    fn test_expand_all() {
        let query = ExpandQuery::all();
        assert_eq!(query.to_query_string(), "$expand=*($levels=1)");
    }

    #[test]
    fn test_expand_current() {
        let query = ExpandQuery::current();
        assert_eq!(query.to_query_string(), "$expand=.($levels=1)");
    }

    #[test]
    fn test_expand_links() {
        let query = ExpandQuery::links();
        assert_eq!(query.to_query_string(), "$expand=~($levels=1)");
    }

    #[test]
    fn test_expand_property() {
        let query = ExpandQuery::property("Thermal");
        assert_eq!(query.to_query_string(), "$expand=Thermal($levels=1)");
    }

    #[test]
    fn test_expand_properties() {
        let query = ExpandQuery::properties(&["Thermal", "Power"]);
        assert_eq!(query.to_query_string(), "$expand=Thermal,Power($levels=1)");
    }

    #[test]
    fn test_expand_with_levels() {
        let query = ExpandQuery::all().levels(3);
        assert_eq!(query.to_query_string(), "$expand=*($levels=3)");
    }

    #[test]
    fn test_simple_eq() {
        let filter = FilterQuery::eq(&"Count", 2);
        assert_eq!(filter.to_query_string(), "$filter=Count eq 2");
    }

    #[test]
    fn test_string_literal() {
        let filter = FilterQuery::eq(&"SystemType", "Physical");
        assert_eq!(filter.to_query_string(), "$filter=SystemType eq 'Physical'");
    }

    #[test]
    fn test_and_operator() {
        let filter = FilterQuery::eq(&"Count", 2)
            .and()
            .eq_then(&"Type", "Physical");
        assert_eq!(
            filter.to_query_string(),
            "$filter=Count eq 2 and Type eq 'Physical'"
        );
    }

    #[test]
    fn test_or_operator() {
        let filter = FilterQuery::eq(&"Count", 2).or().eq_then(&"Count", 4);
        assert_eq!(filter.to_query_string(), "$filter=Count eq 2 or Count eq 4");
    }

    #[test]
    fn test_not_operator() {
        let filter = FilterQuery::eq(&"Count", 2).not();
        assert_eq!(filter.to_query_string(), "$filter=not (Count eq 2)");
    }

    #[test]
    fn test_not_of_compound_expression() {
        let filter = FilterQuery::eq(&"A", 1).and().eq_then(&"B", 2).not();
        assert_eq!(filter.to_query_string(), "$filter=not (A eq 1 and B eq 2)");
    }

    #[test]
    fn test_not_of_group_is_not_double_wrapped() {
        let filter = FilterQuery::eq(&"A", 1).or().eq_then(&"B", 2).group().not();
        assert_eq!(filter.to_query_string(), "$filter=not (A eq 1 or B eq 2)");
    }

    #[test]
    fn test_and_after_or_keeps_builder_order() {
        let filter = FilterQuery::eq(&"A", 1)
            .or()
            .eq_then(&"B", 2)
            .and()
            .eq_then(&"C", 3);
        assert_eq!(
            filter.to_query_string(),
            "$filter=(A eq 1 or B eq 2) and C eq 3"
        );
    }

    #[test]
    fn test_or_after_and_needs_no_parentheses() {
        let filter = FilterQuery::eq(&"A", 1)
            .and()
            .eq_then(&"B", 2)
            .or()
            .eq_then(&"C", 3);
        assert_eq!(
            filter.to_query_string(),
            "$filter=A eq 1 and B eq 2 or C eq 3"
        );
    }

    #[test]
    fn test_grouping() {
        let filter = FilterQuery::eq(&"State", "Enabled")
            .and()
            .eq_then(&"Health", "OK")
            .group()
            .or()
            .eq_then(&"SystemType", "Physical");
        assert_eq!(
            filter.to_query_string(),
            "$filter=(State eq 'Enabled' and Health eq 'OK') or SystemType eq 'Physical'"
        );
    }

    #[test]
    fn test_all_comparison_operators() {
        assert_eq!(FilterQuery::ne(&"A", 1).to_query_string(), "$filter=A ne 1");
        assert_eq!(FilterQuery::gt(&"B", 2).to_query_string(), "$filter=B gt 2");
        assert_eq!(FilterQuery::ge(&"C", 3).to_query_string(), "$filter=C ge 3");
        assert_eq!(FilterQuery::lt(&"D", 4).to_query_string(), "$filter=D lt 4");
        assert_eq!(FilterQuery::le(&"E", 5).to_query_string(), "$filter=E le 5");
    }

    #[test]
    fn test_boolean_literal() {
        let filter = FilterQuery::eq(&"Enabled", true);
        assert_eq!(filter.to_query_string(), "$filter=Enabled eq true");
    }

    #[test]
    fn test_float_literal() {
        let filter = FilterQuery::gt(&"Temperature", 98.6);
        assert_eq!(filter.to_query_string(), "$filter=Temperature gt 98.6");
    }

    #[test]
    fn test_string_escaping() {
        let filter = FilterQuery::eq(&"Name", "O'Brien");
        assert_eq!(filter.to_query_string(), "$filter=Name eq 'O''Brien'");
    }

    #[test]
    fn test_complex_filter() {
        let filter = FilterQuery::eq(&"ProcessorSummary/Count", 2)
            .and()
            .gt_then(&"MemorySummary/TotalSystemMemoryGiB", 64);
        assert_eq!(
            filter.to_query_string(),
            "$filter=ProcessorSummary/Count eq 2 and MemorySummary/TotalSystemMemoryGiB gt 64"
        );
    }
}
