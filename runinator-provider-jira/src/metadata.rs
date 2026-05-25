use runinator_models::providers::{ParameterMetadata, ResultMetadata, RuninatorType};

pub(crate) fn base_param() -> ParameterMetadata {
    ParameterMetadata::required("base_url", RuninatorType::String)
}

pub(crate) fn token_param() -> ParameterMetadata {
    ParameterMetadata::required("token", RuninatorType::String).secret()
}

pub(crate) fn email_param() -> ParameterMetadata {
    ParameterMetadata::optional("email", RuninatorType::String)
}

pub(crate) fn issue_key_param() -> ParameterMetadata {
    ParameterMetadata::required("key", RuninatorType::String)
}

pub(crate) fn jira_results() -> Vec<ResultMetadata> {
    let status_type =
        RuninatorType::open_structure([("name", RuninatorType::String)], RuninatorType::Any);
    let fields_type = RuninatorType::open_structure(
        [
            ("summary", RuninatorType::String),
            ("status", status_type.clone()),
        ],
        RuninatorType::Any,
    );
    let issue_type = RuninatorType::open_structure(
        [
            ("key", RuninatorType::String),
            ("fields", fields_type.clone()),
        ],
        RuninatorType::Any,
    );
    vec![
        ResultMetadata::new("issues", RuninatorType::array(issue_type))
            .with_description("Jira issues returned by search."),
        ResultMetadata::new("key", RuninatorType::String)
            .with_description("Jira issue key returned by issue-oriented APIs."),
        ResultMetadata::new("fields", fields_type).with_description("Selected Jira issue fields."),
        ResultMetadata::new("response", RuninatorType::Any)
            .with_description("Raw Jira API response body."),
    ]
}
