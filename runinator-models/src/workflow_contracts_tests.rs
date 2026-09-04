//! Structural publication compatibility.
use super::*;
use crate::types::RuninatorField;
use std::collections::BTreeMap;

#[test]
fn input_is_contravariant_and_output_covariant() {
    let old: WorkflowDefinition = serde_json::from_value(serde_json::json!({
        "id": null, "name": "test", "input_type": "integer", "output_type": "number"
    }))
    .unwrap();
    let mut new = old.clone();
    new.input_type = RuninatorType::Number;
    new.output_type = RuninatorType::Integer;
    assert_eq!(
        WorkflowContractImpact::compare(Some(&old), &new).compatibility,
        ContractCompatibility::Compatible
    );
    let mut breaking = old.clone();
    breaking.output_type = RuninatorType::Any;
    assert!(WorkflowContractImpact::compare(Some(&old), &breaking).requires_major_bump);
    breaking.version.major += 1;
    assert!(!WorkflowContractImpact::compare(Some(&old), &breaking).requires_major_bump);
}

#[test]
fn open_records_cannot_hide_incompatible_optional_fields() {
    use RuninatorType::*;
    let closed = Struct {
        fields: BTreeMap::from([("x".into(), RuninatorField::optional(String))]),
        additional: None,
    };
    assert!(!contract_assignable(&Map(Box::new(String)), &closed));
    let open = Struct {
        fields: BTreeMap::new(),
        additional: Some(Box::new(Integer)),
    };
    let expected = Struct {
        fields: BTreeMap::from([("x".into(), RuninatorField::optional(String))]),
        additional: Some(Box::new(Any)),
    };
    assert!(!contract_assignable(&open, &expected));
    assert!(!contract_assignable(&Any, &String));
    assert!(contract_assignable(&String, &Any));
}

#[test]
fn containers_unions_ranges_and_functions_are_conservative() {
    use RuninatorType::*;
    assert!(contract_assignable(
        &Union(vec![Integer, String]),
        &Union(vec![String, Number])
    ));
    assert!(contract_assignable(
        &Array(Box::new(Integer)),
        &Array(Box::new(Number))
    ));
    assert!(!contract_assignable(&Number, &Integer));
    let bounded = Range {
        base: Box::new(Integer),
        min: Some(0.into()),
        max: Some(10.into()),
    };
    assert!(contract_assignable(&bounded, &Integer));
    assert!(!contract_assignable(&Integer, &bounded));
    assert!(contract_assignable(
        &Function {
            params: vec![Number],
            ret: Box::new(Integer)
        },
        &Function {
            params: vec![Integer],
            ret: Box::new(Number)
        }
    ));
}

#[test]
fn legacy_metadata_is_used_only_when_the_wire_field_is_absent() {
    let mut value = serde_json::json!({ "id": null, "name": "legacy", "definition": { "metadata": { "rexrap": { "output_type": { "type": "string" } } } } });
    let legacy: WorkflowDefinition = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(legacy.output_type, RuninatorType::String);
    value["output_type"] = serde_json::json!({ "type": "any" });
    let explicit: WorkflowDefinition = serde_json::from_value(value).unwrap();
    assert_eq!(explicit.output_type, RuninatorType::Any);
    assert_eq!(
        serde_json::to_value(&explicit).unwrap()["output_type"],
        serde_json::json!({ "type": "any" })
    );
}
