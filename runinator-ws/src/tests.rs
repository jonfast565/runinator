use crate::repository::*;
use serde_json::json;

#[test]
fn validates_json_schema() {
    let schema = json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "age": { "type": "integer" }
        },
        "required": ["name"]
    });

    assert!(validate_json_schema(&schema, &json!({ "name": "bob", "age": 42 })).is_ok());
    assert!(validate_json_schema(&schema, &json!({ "name": "bob" })).is_ok());
    assert!(validate_json_schema(&schema, &json!({ "age": 42 })).is_err());
    assert!(validate_json_schema(&schema, &json!({ "name": 123 })).is_err());
}

#[test]
fn merges_json_objects() {
    let defaults = json!({ "a": 1, "b": 2 });
    let parameters = json!({ "b": 3, "c": 4 });
    let merged = merge_json_object(&defaults, &parameters);
    assert_eq!(merged, json!({ "a": 1, "b": 3, "c": 4 }));
}

#[test]
fn registered_provider_items_become_provider_metadata() {
    let providers = crate::provider_metadata_from_items(vec![json!({
        "document": {
            "name": "github",
            "actions": [
                { "function_name": "create_pr", "description": "Create a pull request" }
            ]
        }
    })])
    .expect("provider metadata parses");

    assert_eq!(providers[0].name, "github");
    assert_eq!(providers[0].actions[0].function_name, "create_pr");
}

#[test]
fn provider_metadata_becomes_registered_catalog_item() {
    let item = crate::provider_catalog_item(&runinator_models::providers::ProviderMetadata {
        name: "git".into(),
        actions: vec![runinator_models::providers::ActionMetadata::new(
            "diff", "Get diff",
        )],
        metadata: Default::default(),
    });

    assert_eq!(item["item_type"], "provider_metadata");
    assert_eq!(item["document"]["name"], "git");
    assert_eq!(item["document"]["actions"][0]["function_name"], "diff");
}
