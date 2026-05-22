use runinator_models::providers::{ParameterMetadata, ParameterValueType, ResultMetadata};

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
        ResultMetadata::new("response", ParameterValueType::Json)
            .with_description("Raw Jira API response body."),
    ]
}
