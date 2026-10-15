use super::StructDef;
use crate::IsNullable;
use crate::IsRequired;
use crate::OneOrCollection;

use proc_macro2::Literal;
use proc_macro2::TokenStream;
use quote::quote;

#[test]
fn action_parameter_field_generation_combinations() {
    struct TestCase {
        name: &'static str,
        cardinality: OneOrCollection<()>,
        nullable: bool,
        required: bool,
        expected_serde_annotation: TokenStream,
        expected_field_type: TokenStream,
    }

    // Cover the full action-parameter matrix that affects the coordinated
    // serde annotation and Rust field type generation.
    let cases = [
        TestCase {
            name: "required scalar",
            cardinality: OneOrCollection::One(()),
            nullable: false,
            required: true,
            expected_serde_annotation: quote! { #[serde(rename = "TestParam")] },
            expected_field_type: quote! { TestType },
        },
        TestCase {
            name: "required nullable scalar",
            cardinality: OneOrCollection::One(()),
            nullable: true,
            required: true,
            expected_serde_annotation: quote! { #[serde(rename = "TestParam")] },
            expected_field_type: quote! { Option<TestType> },
        },
        TestCase {
            name: "optional scalar",
            cardinality: OneOrCollection::One(()),
            nullable: false,
            required: false,
            expected_serde_annotation: quote! {
                #[serde(rename = "TestParam", skip_serializing_if = "Option::is_none")]
            },
            expected_field_type: quote! { Option<TestType> },
        },
        TestCase {
            name: "optional nullable scalar",
            cardinality: OneOrCollection::One(()),
            nullable: true,
            required: false,
            expected_serde_annotation: quote! {
                #[serde(rename = "TestParam", skip_serializing_if = "Option::is_none")]
            },
            expected_field_type: quote! { Option<Option<TestType>> },
        },
        TestCase {
            name: "required collection",
            cardinality: OneOrCollection::Collection(()),
            nullable: false,
            required: true,
            expected_serde_annotation: quote! { #[serde(rename = "TestParam")] },
            expected_field_type: quote! { Vec<TestType> },
        },
        TestCase {
            name: "required nullable collection",
            cardinality: OneOrCollection::Collection(()),
            nullable: true,
            required: true,
            expected_serde_annotation: quote! { #[serde(rename = "TestParam")] },
            expected_field_type: quote! { Option<Vec<TestType>> },
        },
        TestCase {
            name: "optional collection",
            cardinality: OneOrCollection::Collection(()),
            nullable: false,
            required: false,
            expected_serde_annotation: quote! {
                #[serde(rename = "TestParam", skip_serializing_if = "Option::is_none")]
            },
            expected_field_type: quote! { Option<Vec<TestType>> },
        },
        TestCase {
            name: "optional nullable collection",
            cardinality: OneOrCollection::Collection(()),
            nullable: true,
            required: false,
            expected_serde_annotation: quote! {
                #[serde(rename = "TestParam", skip_serializing_if = "Option::is_none")]
            },
            expected_field_type: quote! { Option<Option<Vec<TestType>>> },
        },
    ];

    for case in cases {
        let field = StructDef::gen_action_parameter_field(
            &case.cardinality,
            quote! { TestType },
            Literal::string("TestParam"),
            IsNullable::new(case.nullable),
            IsRequired::new(case.required),
        );

        assert_token_eq(
            &field.serde_annotation,
            &case.expected_serde_annotation,
            case.name,
            "serde annotation",
        );
        assert_token_eq(
            &field.field_type,
            &case.expected_field_type,
            case.name,
            "field type",
        );
    }
}

fn assert_token_eq(actual: &TokenStream, expected: &TokenStream, case: &str, field: &str) {
    assert_eq!(actual.to_string(), expected.to_string(), "{case}: {field}");
}
