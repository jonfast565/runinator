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

pub(crate) fn comments_results() -> Vec<ResultMetadata> {
    let comment_type = RuninatorType::open_structure(
        [
            ("id", RuninatorType::String),
            ("author", RuninatorType::String),
            ("created", RuninatorType::String),
            ("text", RuninatorType::String),
        ],
        RuninatorType::Any,
    );
    let image_type = RuninatorType::open_structure(
        [
            ("filename", RuninatorType::String),
            ("mime_type", RuninatorType::String),
            ("source_url", RuninatorType::String),
            ("path", RuninatorType::String),
        ],
        RuninatorType::Any,
    );
    vec![
        ResultMetadata::new("text", RuninatorType::String).with_description(
            "All comments rendered to plain text, ready to feed to an AI prompt.",
        ),
        ResultMetadata::new("comments", RuninatorType::array(comment_type))
            .with_description("Parsed comments with author, timestamp, and rendered text."),
        ResultMetadata::new("images", RuninatorType::array(image_type)).with_description(
            "Image attachments downloaded for AI consumption, with local file paths.",
        ),
        ResultMetadata::new("comment_count", RuninatorType::Integer)
            .with_description("Number of comments returned."),
        ResultMetadata::new("image_count", RuninatorType::Integer)
            .with_description("Number of image attachments downloaded."),
    ]
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
