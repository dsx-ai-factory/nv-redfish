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

use nv_redfish_core::Creatable;
use nv_redfish_core::EntityTypeRef;
use nv_redfish_core::ModificationResponse;
use nv_redfish_core::NavProperty;
use nv_redfish_core::ODataId;
use nv_redfish_core::RedfishSettings;
use nv_redfish_core::Reference;
use nv_redfish_core::ReferenceLeaf;
use nv_redfish_core::Updatable;
use nv_redfish_tests::base::expect_root;
use nv_redfish_tests::base::expect_root_srv;
use nv_redfish_tests::base::get_service_root;
use nv_redfish_tests::base::nav_service_root;
use nv_redfish_tests::base::redfish::service_root::ActionType;
use nv_redfish_tests::base::redfish::service_root::ReadOnlyComplexTypeUpdate;
use nv_redfish_tests::base::redfish::service_root::RootSetOnlyComplexType;
use nv_redfish_tests::base::redfish::service_root::ServiceRootUpdate;
use nv_redfish_tests::base::redfish::service_root::TestActionsServiceOemActions;
use nv_redfish_tests::base::redfish::service_root::TestActionsServiceTestSerializationActionAction;
use nv_redfish_tests::base::redfish::service_root::TestCollectionMemberCreate;
use nv_redfish_tests::base::redfish::test_vendor::TestActionsServiceTestActionAction as VendorTestAction;
use nv_redfish_tests::json_merge;
use nv_redfish_tests::Bmc;
use nv_redfish_tests::Error;
use nv_redfish_tests::Expect;
use nv_redfish_tests::ODATA_ID;
use nv_redfish_tests::ODATA_TYPE;
use serde_json::json;
use serde_json::Value;
use tokio::test;

// Check trivial service root retrieval and version read.
#[test]
async fn get_service_root_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let redfish_version = "1.0.0";
    bmc.expect(Expect::get(
        root_id.clone(),
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: &data_type,
            "RedfishVersion": redfish_version,
        }),
    ));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service_root.odata_id(), &root_id);
    assert_eq!(service_root.redfish_version, Some(redfish_version.into()));
    Ok(())
}

// Check that nullable optional property is represent by
// Option<Option<T>> and implementation can distinguish `"field:
// null"` from absense of `field`.
#[test]
async fn optional_nullable_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let property_name = "OptionalNullable";
    let root_id = ODataId::service_root();
    let root_json = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: &data_type,
    });
    bmc.expect(Expect::get(
        root_id.clone(),
        json_merge([&root_json, &json!({ property_name: null })]),
    ));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service_root.optional_nullable, Some(None));

    let value = "Value".to_string();
    bmc.expect(Expect::get(
        root_id.clone(),
        json_merge([&root_json, &json!({ property_name: &value })]),
    ));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service_root.optional_nullable, Some(Some(value)));

    bmc.expect(Expect::get(root_id.clone(), root_json));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service_root.optional_nullable, None);
    Ok(())
}

// Check that rigid array property accepts both regular and null-containing arrays.
#[test]
async fn rigid_array_read_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let property_name = "RigidArrayValues";
    let root_id = ODataId::service_root();
    let root_json = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: &data_type,
    });

    bmc.expect(Expect::get(
        root_id.clone(),
        json_merge([&root_json, &json!({ property_name: ["a", "b", "c"] })]),
    ));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(
        service_root.rigid_array_values,
        Some(vec![Some("a".into()), Some("b".into()), Some("c".into())])
    );

    bmc.expect(Expect::get(
        root_id.clone(),
        json_merge([&root_json, &json!({ property_name: ["a", null, "c"] })]),
    ));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(
        service_root.rigid_array_values,
        Some(vec![Some("a".into()), None, Some("c".into())])
    );

    Ok(())
}

// Check service with required property.
#[test]
async fn required_non_nullable_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestRequiredService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");

    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert!(matches!(
        service_root.test_required_service.as_ref(),
        Some(NavProperty::Reference(_))
    ));

    let value = "SomeValue".to_string();
    bmc.expect(Expect::get(
        &service_id,
        &json!({
            ODATA_ID: &service_id,
            ODATA_TYPE: &service_data_type,
            "Required": &value,
        }),
    ));

    let service = service_root
        .test_required_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_required_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;
    assert_eq!(service.required, value);
    Ok(())
}

// Check that nullable optional property is represent by
// Option<Option<T>> and implementation can distinguish `"field:
// null"` from absense of `field`.
#[test]
async fn required_nullable_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestRequiredNullableService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");
    let property_name = "RequiredNullable";
    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    assert!(matches!(
        service_root.test_required_nullable_service.as_ref(),
        Some(NavProperty::Reference(_))
    ));

    let service_tpl = json!({
        ODATA_ID: &service_id,
        ODATA_TYPE: &service_data_type,
    });

    bmc.expect(Expect::get(
        &service_id,
        json_merge([&service_tpl, &json!({ property_name: null })]),
    ));
    let service = service_root
        .test_required_nullable_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_nullable_required_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;
    assert_eq!(service.required_nullable, None);

    let value = "SomeValue".to_string();
    bmc.expect(Expect::get(
        service_id.clone(),
        json_merge([&service_tpl, &json!({ property_name: &value })]),
    ));
    let service = service.refresh(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service.required_nullable, Some(value));

    bmc.expect(Expect::get(service_id.clone(), &service_tpl));
    assert!(service.refresh(&bmc).await.map_err(Error::Bmc).is_err());
    Ok(())
}

// Check that nullable optional property is represent by
// Option<Option<T>> and implementation can distinguish `"field:
// null"` from absense of `field`.
#[test]
async fn required_collection_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestRequiredCollectionService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");
    let property_name = "RequiredCollection";
    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    assert!(matches!(
        service_root.test_required_collection_service.as_ref(),
        Some(NavProperty::Reference(_))
    ));

    let service_tpl = json!({
        ODATA_ID: &service_id,
        ODATA_TYPE: &service_data_type,
    });

    let empty = Vec::<String>::new();
    bmc.expect(Expect::get(
        &service_id,
        json_merge([&service_tpl, &json!({ property_name: empty })]),
    ));
    let service = service_root
        .test_required_collection_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_nullable_required_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;
    assert_eq!(service.required_collection, empty);

    let value = vec!["SomeValue1".to_string(), "SomeValue2".to_string()];
    bmc.expect(Expect::get(
        service_id.clone(),
        json_merge([&service_tpl, &json!({ property_name: value })]),
    ));
    let service = service.refresh(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service.required_collection, value);

    Ok(())
}

// Check updatable for properties.
#[test]
async fn update_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let updatable_name = "Updatable";
    let updatable_guid_name = "UpdatableGuid";
    let write_only_name = "WriteOnly";
    let root_id = ODataId::service_root();
    let root_json = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: &data_type,
    });
    bmc.expect(expect_root());
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(service_root.updatable, None);

    let uuid_str = "a1a2a3a4-b1b2-c1c2-d1d2-d3d4d5d6d7d8";
    let uuid_value = uuid_str.parse().expect("uuid must be parsed");
    let value = "Value".to_string();
    bmc.expect(Expect::update(
        root_id.clone(),
        json!({ updatable_name: &value, updatable_guid_name: &uuid_str }),
        &json_merge([
            &root_json,
            &json!({ updatable_name: &value, updatable_guid_name: &uuid_str}),
        ]),
    ));
    let service_root = service_root
        .update(
            &bmc,
            &ServiceRootUpdate {
                // Here we actually checks that update struct doesn't include:
                // 1. read-only fields (like redfish_version)
                //
                // If this code compiles then check passed.
                updatable: Some(value.clone()),
                read_only_complex: None,
                rigid_array_values: None,
                updatable_guid: Some(uuid_value),
                write_only: None,
            },
        )
        .await
        .map_err(Error::Bmc)?;
    let service_root = match service_root {
        ModificationResponse::Entity(service_root) => service_root,
        _ => return Err(Error::ExpectedProperty("service_root")),
    };
    assert_eq!(service_root.updatable, Some(value));
    assert_eq!(service_root.updatable_guid, Some(uuid_value));

    // Update write only:
    let value = "Value".to_string();
    bmc.expect(Expect::update(
        root_id.clone(),
        json!({ write_only_name: &value }),
        json_merge([&root_json, &json!({})]),
    ));

    let response = service_root
        .update(
            &bmc,
            &ServiceRootUpdate {
                updatable: None,
                read_only_complex: None,
                rigid_array_values: None,
                updatable_guid: None,
                write_only: Some(value.clone()),
            },
        )
        .await
        .map_err(Error::Bmc)?;

    assert!(matches!(response, ModificationResponse::Entity(_)));

    Ok(())
}

// Check updatable for navigation property.
#[test]
async fn update_using_nav_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let updatable_name = "Updatable";
    let root_id = ODataId::service_root();
    let root_json = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: &data_type,
    });
    let nav_service_root = nav_service_root();
    let value = "Value".to_string();
    bmc.expect(Expect::update(
        root_id.clone(),
        json!({ updatable_name: &value }),
        &json_merge([&root_json, &json!({ updatable_name: &value })]),
    ));
    let nav_service_root = nav_service_root
        .update(
            &bmc,
            &ServiceRootUpdate {
                updatable: Some(value.clone()),
                read_only_complex: None,
                rigid_array_values: None,
                updatable_guid: None,
                write_only: None,
            },
        )
        .await
        .map_err(Error::Bmc)?;
    let nav_service_root = match nav_service_root {
        ModificationResponse::Entity(nav_service_root) => nav_service_root,
        _ => return Err(Error::ExpectedProperty("nav_service_root")),
    };
    assert_eq!(
        nav_service_root
            .get(&bmc)
            .await
            .expect("no requests created")
            .updatable,
        Some(value)
    );
    Ok(())
}

// Check update payload and refresh behavior for rigid arrays.
#[test]
async fn update_rigid_array_property_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let property_name = "RigidArrayValues";
    let root_id = ODataId::service_root();
    let root_json = json!({
        ODATA_ID: &root_id,
        ODATA_TYPE: &data_type,
    });
    bmc.expect(expect_root());
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    let updated_payload = vec![Some("a".to_string()), None];
    bmc.expect(Expect::update(
        root_id.clone(),
        json!({ property_name: ["a", null] }),
        &json_merge([&root_json, &json!({ property_name: ["a", null] })]),
    ));
    let service_root = service_root
        .update(
            &bmc,
            &ServiceRootUpdate {
                updatable: None,
                read_only_complex: None,
                rigid_array_values: Some(updated_payload.clone()),
                updatable_guid: None,
                write_only: None,
            },
        )
        .await
        .map_err(Error::Bmc)?;
    let service_root = match service_root {
        ModificationResponse::Entity(service_root) => service_root,
        _ => return Err(Error::ExpectedProperty("service_root")),
    };
    assert_eq!(
        service_root.rigid_array_values,
        Some(updated_payload.clone())
    );

    // Ensure field is omitted when not set in update struct.
    bmc.expect(Expect::update(root_id.clone(), json!({}), &root_json));
    let response = service_root
        .update(
            &bmc,
            &ServiceRootUpdate {
                updatable: None,
                read_only_complex: None,
                rigid_array_values: None,
                updatable_guid: None,
                write_only: None,
            },
        )
        .await
        .map_err(Error::Bmc)?;

    assert!(matches!(response, ModificationResponse::Entity(_)));

    // Refresh keeps null element in rigid array payload.
    bmc.expect(Expect::get(
        root_id.clone(),
        json_merge([&root_json, &json!({ property_name: ["a", null] })]),
    ));
    let refreshed = service_root.refresh(&bmc).await.map_err(Error::Bmc)?;
    assert_eq!(
        refreshed.rigid_array_values,
        Some(vec![Some("a".into()), None])
    );

    Ok(())
}

// Every `TestCases` spawns its own nested cargo build against trybuild's
// separate target directory, which for this crate means rebuilding nv-redfish
// and its dependencies from scratch. One batch keeps that to a single build.
#[test]
async fn compile_fails() {
    let t = trybuild::TestCases::new();
    // Write only must not be generated in read structures.
    t.compile_fail("tests/compile-fails/no-write-only-in-read.rs");
    // Action parameters can contain sensitive information and must not implement Debug until the
    // schema compiler can explicitly identify and redact sensitive fields.
    t.compile_fail("tests/compile-fails/no-debug-for-action-parameters.rs");
}

// Check that collection provides create method.
#[test]
async fn create_collection_member_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let collection_name = "TestCollection";
    let collection_id = format!("{root_id}/{collection_name}");
    let collection_data_type = format!("ServiceRoot.v1_0_0.{collection_name}");
    bmc.expect(expect_root_srv(collection_name, &collection_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    assert!(matches!(
        service_root.test_collection.as_ref(),
        Some(NavProperty::Reference(_))
    ));

    let collection_tpl = json!({
        ODATA_ID: &collection_id,
        ODATA_TYPE: &collection_data_type,
    });
    bmc.expect(Expect::get(
        &collection_id,
        json_merge([&collection_tpl, &json!({ "Members": [] })]),
    ));
    let collection = service_root
        .test_collection
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_collection"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;

    let collection_member_id = format!("{root_id}/{collection_name}/1");
    let collection_member_data_type = format!("ServiceRoot.v1_0_0.TestCollectionMember");
    let collection_member_tpl = json!({
        ODATA_ID: &collection_member_id,
        ODATA_TYPE: &collection_member_data_type,
    });
    bmc.expect(Expect::create(
        &collection_id,
        json!({
            "RequiredOnCreate": "required value",
            "ReadOnlyComplex": {
                "Required": "nested required value",
            },
        }),
        collection_member_tpl,
    ));
    let member = collection
        .create(
            &bmc,
            &TestCollectionMemberCreate::builder(
                "required value".into(),
                ReadOnlyComplexTypeUpdate::builder()
                    .with_required("nested required value".into())
                    .build(),
            )
            .build(),
        )
        .await
        .map_err(Error::Bmc)?;
    let member = match member {
        ModificationResponse::Entity(member) => member,
        _ => return Err(Error::ExpectedProperty("member")),
    };
    assert_eq!(member.odata_id().to_string(), collection_member_id);
    Ok(())
}

#[test]
async fn create_struct_required_on_create_and_writable_fields_test() -> Result<(), Error> {
    let create = TestCollectionMemberCreate::builder(
        "required value".into(),
        ReadOnlyComplexTypeUpdate::builder()
            .with_required("nested required value".into())
            .build(),
    )
    .with_optional_writable("optional value".into())
    .build();

    assert_eq!(
        serde_json::to_value(create).expect("serializable"),
        json!({
            "RequiredOnCreate": "required value",
            "ReadOnlyComplex": {
                "Required": "nested required value",
            },
            "OptionalWritable": "optional value",
        })
    );
    Ok(())
}

// A vendor schema binds its own `TestAction` to the OEM extension
// point: the generated struct must live in the vendor's module
// (not collide with the standard `TestAction`) and its field must
// serialize under the vendor's namespace.
#[test]
async fn oem_action_disambiguation_test() {
    let oem_actions: TestActionsServiceOemActions = serde_json::from_value(json!({
        "#TestVendor.TestAction": {
            "target": "/redfish/v1/TestActionsService/Actions/Oem/TestVendor.TestAction"
        }
    }))
    .expect("vendor action deserializes under its own namespace");
    assert!(oem_actions.test_action.is_some());
    let _distinct_from_standard: VendorTestAction = VendorTestAction {};
}

// Check that actions method.
#[test]
async fn action_method_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestActionsService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");
    // Actions key by their defining schema's namespace (the fixture
    // defines TestAction inside ServiceRoot), not the entity name.
    let action_field = "#ServiceRoot.TestAction";
    let action_target = format!("{root_id}/{service_name}/Actions/ServiceRoot.TestAction");
    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    assert!(matches!(
        service_root.test_actions_service.as_ref(),
        Some(NavProperty::Reference(_))
    ));

    let service_tpl = json!({
        ODATA_ID: &service_id,
        ODATA_TYPE: &service_data_type,
    });
    bmc.expect(Expect::get(
        &service_id,
        json_merge([
            &service_tpl,
            &json!({
                "Actions": {
                    action_field: {
                        "target": action_target
                    }
                }
            }),
        ]),
    ));
    let service = service_root
        .test_actions_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_actions_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;

    let service_actions = service
        .actions
        .as_ref()
        .ok_or(Error::ExpectedProperty("actions"))?;

    bmc.expect(Expect::action(
        &action_target,
        json!({
            "ActionType": "Option1"
        }),
        &json!(null),
    ));

    assert!(matches!(
        service_actions
            .test_action(&bmc, ActionType::Option1)
            .await
            .map_err(Error::Bmc)?,
        ModificationResponse::Entity(())
    ));

    Ok(())
}

#[test]
async fn action_parameter_serialization_test() -> Result<(), Error> {
    struct TestCase {
        name: &'static str,
        request: TestActionsServiceTestSerializationActionAction,
        expected: Value,
    }

    fn reference(id: &str) -> Reference {
        Reference::from(&ReferenceLeaf {
            odata_id: id.to_string().into(),
        })
    }

    let cases = [
        TestCase {
            name: "optional fields are omitted and required nullable null is present",
            request: TestActionsServiceTestSerializationActionAction {
                required_value: "required".into(),
                required_nullable_value: None,
                required_collection: vec!["required collection".into()],
                required_nullable_collection: None,
                optional_value: None,
                optional_nullable_value: None,
                optional_collection: None,
                optional_nullable_collection: None,
                optional_nullable_entity: None,
            },
            expected: json!({
                "RequiredValue": "required",
                "RequiredNullableValue": null,
                "RequiredCollection": ["required collection"],
                "RequiredNullableCollection": null,
            }),
        },
        TestCase {
            name: "optional nullable fields can serialize explicit null",
            request: TestActionsServiceTestSerializationActionAction {
                required_value: "required".into(),
                required_nullable_value: Some("required nullable".into()),
                required_collection: vec!["required collection".into()],
                required_nullable_collection: Some(vec!["required nullable collection".into()]),
                optional_value: None,
                optional_nullable_value: Some(None),
                optional_collection: None,
                optional_nullable_collection: Some(None),
                optional_nullable_entity: Some(None),
            },
            expected: json!({
                "RequiredValue": "required",
                "RequiredNullableValue": "required nullable",
                "RequiredCollection": ["required collection"],
                "RequiredNullableCollection": ["required nullable collection"],
                "OptionalNullableValue": null,
                "OptionalNullableCollection": null,
                "OptionalNullableEntity": null,
            }),
        },
        TestCase {
            name: "optional fields serialize values when present",
            request: TestActionsServiceTestSerializationActionAction {
                required_value: "required".into(),
                required_nullable_value: Some("required nullable".into()),
                required_collection: vec!["required collection".into()],
                required_nullable_collection: Some(vec!["required nullable collection".into()]),
                optional_value: Some("optional".into()),
                optional_nullable_value: Some(Some("optional nullable".into())),
                optional_collection: Some(vec!["a".into(), "b".into()]),
                optional_nullable_collection: Some(Some(vec!["c".into(), "d".into()])),
                optional_nullable_entity: Some(Some(reference("/redfish/v1/TestRequiredService"))),
            },
            expected: json!({
                "RequiredValue": "required",
                "RequiredNullableValue": "required nullable",
                "RequiredCollection": ["required collection"],
                "RequiredNullableCollection": ["required nullable collection"],
                "OptionalValue": "optional",
                "OptionalNullableValue": "optional nullable",
                "OptionalCollection": ["a", "b"],
                "OptionalNullableCollection": ["c", "d"],
                "OptionalNullableEntity": {
                    "@odata.id": "/redfish/v1/TestRequiredService",
                },
            }),
        },
    ];

    for case in cases {
        assert_eq!(
            serde_json::to_value(&case.request).expect("action request should serialize"),
            case.expected,
            "{}",
            case.name
        );
    }

    Ok(())
}

// Deserialize @Redfish.Settings and navigate to settings object.
#[test]
async fn redfish_settings_nav_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestSettingsService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");

    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    let settings_id = format!("{service_id}/Settings");
    bmc.expect(Expect::get(
        &service_id,
        json!({
            ODATA_ID: &service_id,
            ODATA_TYPE: &service_data_type,
            "@Redfish.Settings": { "SettingsObject": { ODATA_ID: &settings_id } },
            "@Redfish.SettingsApplyTime": {},
            "SettingValue": "current",
        }),
    ));
    let service = service_root
        .test_settings_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_settings_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;

    assert!(service.redfish_settings.is_some());
    assert!(service.redfish_settings_apply_type.is_some());
    let settings_nav = service.settings_object().expect("settings nav must exist");

    // Fetch settings object
    bmc.expect(Expect::get(
        &settings_id,
        json!({
            ODATA_ID: &settings_id,
            ODATA_TYPE: &service_data_type,
            "SettingValue": "current",
        }),
    ));
    let _settings = settings_nav.get(&bmc).await.map_err(Error::Bmc)?;
    Ok(())
}

// Update via settings object; ensure update goes to settings resource id and applies value.
#[test]
async fn redfish_settings_update_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestSettingsService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");

    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    let settings_id = format!("{service_id}/Settings");
    bmc.expect(Expect::get(
        &service_id,
        json!({
            ODATA_ID: &service_id,
            ODATA_TYPE: &service_data_type,
            "@Redfish.Settings": { "SettingsObject": { ODATA_ID: &settings_id } },
            "@Redfish.SettingsApplyTime": {},
            "SettingValue": "current",
        }),
    ));
    let service = service_root
        .test_settings_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_settings_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;

    let settings_nav = service.settings_object().expect("settings nav must exist");
    // Retrieve settings object once and update it
    bmc.expect(Expect::get(
        &settings_id,
        json!({
            ODATA_ID: &settings_id,
            ODATA_TYPE: &service_data_type,
            "SettingValue": "current",
        }),
    ));
    let settings = settings_nav.get(&bmc).await.map_err(Error::Bmc)?;

    let new_value = "new".to_string();
    bmc.expect(Expect::update(
        &settings_id,
        json!({ "SettingValue": &new_value }),
        json!({
            ODATA_ID: &settings_id,
            ODATA_TYPE: &service_data_type,
            "SettingValue": &new_value,
        }),
    ));
    let updated = settings
        .update(
            &bmc,
            &nv_redfish_tests::base::redfish::service_root::TestSettingsServiceUpdate {
                setting_value: Some(new_value.clone()),
            },
        )
        .await
        .map_err(Error::Bmc)?;
    let updated = match updated {
        ModificationResponse::Entity(updated) => updated,
        _ => return Err(Error::ExpectedProperty("updated")),
    };
    assert_eq!(updated.setting_value, Some(Some(new_value)));
    Ok(())
}

// If no @Redfish.Settings present, settings_object() returns None.
#[test]
async fn redfish_settings_absent_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();
    let service_name = "TestSettingsService";
    let service_id = format!("{root_id}/{service_name}");
    let service_data_type = format!("ServiceRoot.v1_0_0.{service_name}");

    bmc.expect(expect_root_srv(service_name, &service_id));
    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;

    // No settings annotations included
    bmc.expect(Expect::get(
        &service_id,
        json!({
            ODATA_ID: &service_id,
            ODATA_TYPE: &service_data_type,
            "SettingValue": "current",
        }),
    ));
    let service = service_root
        .test_settings_service
        .as_ref()
        .ok_or(Error::ExpectedProperty("test_settings_service"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;
    assert!(service.settings_object().is_none());
    Ok(())
}

// Excerpt view tests: verify inline excerpt copies and direct read
#[test]
async fn excerpt_views_test() -> Result<(), Error> {
    let bmc = Bmc::default();
    let root_id = ODataId::service_root();

    // Expect root with links to new services (both in a single response)
    let data_type = "ServiceRoot.v1_0_0.ServiceRoot";
    let excerpt_entity_id = format!("{}/ExcerptEntity", root_id);
    let excerpt_ref_entity_id = format!("{}/ExcerptRefEntity", root_id);
    bmc.expect(Expect::get(
        root_id.clone(),
        json!({
            ODATA_ID: &root_id,
            ODATA_TYPE: &data_type,
            "ExcerptEntity": { ODATA_ID: &excerpt_entity_id },
            "ExcerptRefEntity": { ODATA_ID: &excerpt_ref_entity_id },
        }),
    ));

    let service_root = get_service_root(&bmc).await.map_err(Error::Bmc)?;
    assert!(matches!(
        service_root.excerpt_entity.as_ref(),
        Some(NavProperty::Reference(_))
    ));
    assert!(matches!(
        service_root.excerpt_ref_entity.as_ref(),
        Some(NavProperty::Reference(_))
    ));

    // Fetch ExcerptRefEntity and validate inline excerpts
    let ref_id = format!("{}/ExcerptRefEntity", root_id);
    let ref_dt = "ServiceRoot.v1_0_0.ExcerptRefEntity";
    let all = json!({
      "Always": "A",
      "BasicProp": "B",
      "DetailsProp": "D"
    });
    let basic = json!({
      "Always": "A",
      "BasicProp": "B"
    });
    let details = json!({
      "Always": "A",
      "DetailsProp": "D"
    });

    bmc.expect(Expect::get(
        &ref_id,
        json!({
          ODATA_ID: &ref_id,
          ODATA_TYPE: ref_dt,
          "ExcerptAll": all,
          "ExcerptBasic": basic,
          "ExcerptDetails": details,
        }),
    ));
    let ref_svc = service_root
        .excerpt_ref_entity
        .as_ref()
        .ok_or(Error::ExpectedProperty("excerpt_ref_entity"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;
    assert_eq!(
        ref_svc.excerpt_all.as_ref().unwrap().always,
        Some("A".into())
    );
    assert_eq!(
        ref_svc.excerpt_basic.as_ref().unwrap().basic_prop,
        Some("B".into())
    );
    assert_eq!(
        ref_svc.excerpt_details.as_ref().unwrap().details_prop,
        Some("D".into())
    );

    // Fetch ExcerptEntity directly and verify full entity contains Hidden
    let tgt_id = format!("{}/ExcerptEntity", root_id);
    let tgt_dt = "ServiceRoot.v1_0_0.ExcerptEntity";
    bmc.expect(Expect::get(
        &tgt_id,
        json!({
          ODATA_ID: &tgt_id,
          ODATA_TYPE: tgt_dt,
          "Always": "A",
          "BasicProp": "B",
          "DetailsProp": "D",
          "Hidden": "H"
        }),
    ));
    let tgt = service_root
        .excerpt_entity
        .as_ref()
        .ok_or(Error::ExpectedProperty("excerpt_entity"))?
        .get(&bmc)
        .await
        .map_err(Error::Bmc)?;
    assert_eq!(tgt.hidden, Some("H".into()));

    Ok(())
}

// Check that generated enums accept unknown values via fallback variant.
#[test]
async fn enum_unknown_value_falls_back_to_unsupported_value() {
    let known: ActionType =
        serde_json::from_value(json!("Option1")).expect("known enum value must deserialize");
    assert_eq!(known, ActionType::Option1);

    let unknown: ActionType = serde_json::from_value(json!("FutureOption"))
        .expect("unknown enum value must deserialize to fallback");
    assert_eq!(unknown, ActionType::UnsupportedValue);

    let serialized =
        serde_json::to_value(ActionType::UnsupportedValue).expect("fallback must serialize");
    assert_eq!(serialized, json!("UnsupportedValue"));
}

// Check that standalone complex types matched by root set patterns are generated.
#[test]
async fn root_set_complex_type_is_generated_test() {
    let value: RootSetOnlyComplexType = serde_json::from_value(json!({
        "Value": "root-set complex",
    }))
    .expect("root-set complex type must deserialize");

    assert_eq!(value.value, Some("root-set complex".into()));
}
