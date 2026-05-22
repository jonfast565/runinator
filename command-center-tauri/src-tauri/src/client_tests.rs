use super::*;

#[test]
fn command_url_preserves_query() {
    let url = build_url("http://localhost:3000/api/", "runs/7/chunks?limit=500").unwrap();
    assert_eq!(
        url.as_str(),
        "http://localhost:3000/api/runs/7/chunks?limit=500"
    );
}

#[test]
fn extracts_json_error_message() {
    assert_eq!(
        extract_error_message(r#"{"message":"failed cleanly"}"#),
        Some("failed cleanly".to_string())
    );
}

#[test]
fn ignores_empty_json_error_message() {
    assert_eq!(extract_error_message(r#"{"message":""}"#), None);
}
