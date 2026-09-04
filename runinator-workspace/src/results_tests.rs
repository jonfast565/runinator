//! Result references preserve types and fail before execution on missing data.
use super::*;
use runinator_models::json;

#[test]
fn resolves_nested_references_without_interpreting_saved_content() {
    let results = BTreeMap::from([(
        "answer".into(),
        json!({"items": [42], "literal": {"$workspace": "/missing"}}),
    )]);
    assert_eq!(resolve_results(&json!({"input": [{"$workspace": "/answer/items/0"}], "literal": {"$workspace": "/answer/literal"}}), Some(&results)).unwrap(),
        json!({"input": [42], "literal": {"$workspace": "/missing"}}));
}

#[test]
fn rejects_missing_or_malformed_references() {
    for input in [
        json!({"$workspace": "/missing"}),
        json!({"$workspace": 42}),
        json!({"$workspace": "", "extra": true}),
    ] {
        assert!(resolve_results(&input, Some(&BTreeMap::new())).is_err());
    }
    assert!(resolve_results(&json!({"$workspace": ""}), None).is_err());
    assert_eq!(
        resolve_results(&json!({"value": 42}), None).unwrap(),
        json!({"value": 42})
    );
}
