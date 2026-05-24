use runinator_models::providers::{ParameterMetadata, ParameterValueType, ResultMetadata};
use serde_json::json;

pub(crate) fn base_param() -> ParameterMetadata {
    ParameterMetadata::required("base_url", ParameterValueType::String)
}

pub(crate) fn token_param() -> ParameterMetadata {
    ParameterMetadata::required("token", ParameterValueType::String).secret()
}

pub(crate) fn email_param() -> ParameterMetadata {
    ParameterMetadata::optional("email", ParameterValueType::String)
}

pub(crate) fn issue_key_param() -> ParameterMetadata {
    ParameterMetadata::required("key", ParameterValueType::String)
}

pub(crate) fn jira_results() -> Vec<ResultMetadata> {
    vec![
        ResultMetadata::new("issues", ParameterValueType::Json)
            .with_schema(json!({
                "type": "array",
                "items": jira_issue_schema()
            }))
            .with_description("Jira issues returned by search."),
        ResultMetadata::new("key", ParameterValueType::String)
            .with_description("Jira issue key returned by issue-oriented APIs."),
        ResultMetadata::new("fields", ParameterValueType::Object)
            .with_schema(json!({
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "status": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                }
            }))
            .with_description("Selected Jira issue fields."),
        ResultMetadata::new("response", ParameterValueType::Json)
            .with_description("Raw Jira API response body."),
    ]
}

fn jira_issue_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "key": { "type": "string" },
            "fields": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "status": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        }
                    }
                }
            }
        }
    })
}
