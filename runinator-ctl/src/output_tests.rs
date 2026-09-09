//! Covers the common human-facing table projection for serialized CLI results.

use super::*;

#[test]
fn object_values_render_as_field_value_rows() {
    let table = json_value_table(&serde_json::json!({
        "id": "run-123",
        "enabled": true,
        "metadata": { "region": "us-east-1" },
    }));

    assert!(table.starts_with("field"));
    assert!(table.contains("id"));
    assert!(table.contains("run-123"));
    assert!(table.contains("{\"region\":\"us-east-1\"}"));
}

#[test]
fn object_lists_render_each_item_as_a_table_row() {
    let table = json_value_table(&serde_json::json!([
        { "id": "one", "status": "ready" },
        { "id": "two", "status": "waiting", "owner": "ops" },
    ]));

    assert!(table.starts_with("id"));
    assert!(table.contains("status"));
    assert!(table.contains("owner"));
    assert!(table.contains("one"));
    assert!(table.contains("two"));
}

#[test]
fn empty_lists_keep_a_table_header() {
    let table = json_value_table(&serde_json::json!([]));

    assert_eq!(table, "value\n");
}
