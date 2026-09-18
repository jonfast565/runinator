//! Shared typed Jira API clients.

pub mod errors;

mod async_jira_client;
mod jira_client;
mod jira_credentials;
mod request_spec;

use reqwest::{Method, Url};
use serde_json::{Value, json};

use errors::JiraError;
use request_spec::RequestSpec;

pub use async_jira_client::AsyncJiraClient;
pub use jira_client::JiraClient;
pub use jira_credentials::JiraCredentials;

pub enum JiraOperation {
    Search {
        jql: String,
        fields: String,
        max_results: Option<u32>,
        next_page_token: Option<String>,
    },
    Issue {
        key: String,
        fields: Option<String>,
    },
    Comments {
        key: String,
        start_at: u64,
        max_results: u32,
    },
    AddComment {
        key: String,
        body: Value,
    },
    Transition {
        key: String,
        transition_id: String,
        update: Option<Value>,
    },
    Download {
        url: String,
    },
}

impl JiraOperation {
    fn request(self, base_url: &Url) -> Result<RequestSpec, JiraError> {
        use JiraOperation::*;
        match self {
            Search {
                jql,
                fields,
                max_results,
                next_page_token,
            } => {
                let mut spec = get(base_url
                    .join("rest/api/3/search/jql")
                    .map_err(JiraError::config)?);
                spec.query
                    .extend([("jql".into(), jql), ("fields".into(), fields)]);
                if let Some(value) = max_results {
                    spec.query.push(("maxResults".into(), value.to_string()));
                }
                if let Some(value) = next_page_token {
                    spec.query.push(("nextPageToken".into(), value));
                }
                Ok(spec)
            }
            Issue { key, fields } => {
                let mut spec = get(base_url
                    .join(&format!("rest/api/3/issue/{key}"))
                    .map_err(JiraError::config)?);
                if let Some(fields) = fields {
                    spec.query.push(("fields".into(), fields));
                }
                Ok(spec)
            }
            Comments {
                key,
                start_at,
                max_results,
            } => {
                let mut spec = get(base_url
                    .join(&format!("rest/api/3/issue/{key}/comment"))
                    .map_err(JiraError::config)?);
                spec.query.extend([
                    ("startAt".into(), start_at.to_string()),
                    ("maxResults".into(), max_results.to_string()),
                ]);
                Ok(spec)
            }
            AddComment { key, body } => Ok(post(
                base_url
                    .join(&format!("rest/api/3/issue/{key}/comment"))
                    .map_err(JiraError::config)?,
                body,
            )),
            Transition {
                key,
                transition_id,
                update,
            } => {
                let mut body = json!({ "transition": { "id": transition_id } });
                if let Some(update) = update {
                    body["update"] = update;
                }
                Ok(post(
                    base_url
                        .join(&format!("rest/api/3/issue/{key}/transitions"))
                        .map_err(JiraError::config)?,
                    body,
                ))
            }
            Download { url } => Ok(get(Url::parse(&url).map_err(JiraError::config)?)),
        }
    }
}

pub fn validate_base_url(base_url: &str) -> Result<Url, JiraError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err(JiraError::config(
            "jira base_url is empty; set config.jira.base_url to your Jira site URL",
        ));
    }
    let mut url = Url::parse(trimmed).map_err(|error| {
        JiraError::config(format!(
            "jira base_url \"{base_url}\" is not a valid URL: {error}"
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(JiraError::config(format!(
            "jira base_url \"{base_url}\" has unsupported scheme \"{}\"; expected http or https",
            url.scheme()
        )));
    }
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(url)
}

fn get(url: Url) -> RequestSpec {
    RequestSpec {
        method: Method::GET,
        url,
        query: Vec::new(),
        body: None,
    }
}
fn post(url: Url, body: Value) -> RequestSpec {
    RequestSpec {
        method: Method::POST,
        url,
        query: Vec::new(),
        body: Some(body),
    }
}

/// Render an Atlassian Document Format value as readable plain text.
pub fn render_comment_body(body: Option<&Value>) -> String {
    match body {
        Some(Value::String(value)) => value.trim().to_string(),
        Some(node @ Value::Object(_)) => {
            let mut output = String::new();
            walk_adf(node, &mut output);
            collapse_blank_lines(&output)
        }
        _ => String::new(),
    }
}

fn walk_adf(node: &Value, output: &mut String) {
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or_default();
    match node_type {
        "text" => {
            output.push_str(node.get("text").and_then(Value::as_str).unwrap_or_default());
            return;
        }
        "hardBreak" => {
            output.push('\n');
            return;
        }
        "media" | "mediaInline" => {
            let alt = node
                .pointer("/attrs/alt")
                .or_else(|| node.pointer("/attrs/__fileName"))
                .and_then(Value::as_str)
                .unwrap_or("image");
            output.push_str(&format!("[image: {alt}]"));
            return;
        }
        "mention" => {
            output.push_str(
                node.pointer("/attrs/text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            );
            return;
        }
        "listItem" => output.push_str("- "),
        _ => {}
    }
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for child in children {
            walk_adf(child, output);
        }
    }
    if matches!(
        node_type,
        "paragraph" | "heading" | "blockquote" | "listItem" | "mediaSingle" | "codeBlock" | "rule"
    ) {
        output.push('\n');
    }
}

fn collapse_blank_lines(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut newline_run = 0;
    for character in value.chars() {
        if character == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                output.push(character);
            }
        } else {
            newline_run = 0;
            output.push(character);
        }
    }
    output.trim().to_string()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn server(request_count: usize) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().unwrap();
                let mut bytes = [0; 4096];
                let count = stream.read(&mut bytes).unwrap();
                sender
                    .send(String::from_utf8_lossy(&bytes[..count]).into_owned())
                    .unwrap();
                let body = r#"{"id":"10"}"#;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (format!("http://{address}"), receiver)
    }

    #[test]
    fn operations_own_jira_endpoint_construction() {
        let base = validate_base_url("https://acme.atlassian.net").unwrap();
        let spec = JiraOperation::Search {
            jql: "project = RUNI".into(),
            fields: "summary".into(),
            max_results: Some(100),
            next_page_token: Some("next".into()),
        }
        .request(&base)
        .unwrap();
        assert_eq!(spec.url.path(), "/rest/api/3/search/jql");
        assert_eq!(
            spec.query.last(),
            Some(&("nextPageToken".into(), "next".into()))
        );
    }

    #[test]
    fn consumers_do_not_construct_jira_calls_directly() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        for relative in [
            "runinator-provider-jira/src/provider.rs",
            "runinator-provider-jira/src/search.rs",
            "runinator-provider-jira/src/comments.rs",
            "runinator-adapter-host/src/main.rs",
        ] {
            let source = std::fs::read_to_string(root.join(relative)).unwrap();
            assert!(
                !source.contains("/rest/api/3"),
                "{relative} constructs a Jira URL"
            );
            assert!(
                !source.contains("reqwest::"),
                "{relative} calls Jira through reqwest"
            );
        }
    }

    #[tokio::test]
    async fn blocking_and_async_clients_share_operation_behavior() {
        let (root, requests) = server(2);
        let credentials = JiraCredentials {
            email: "dev@example.com".into(),
            token: "secret".into(),
        };
        let asynchronous =
            AsyncJiraClient::new(&root, credentials.clone(), Duration::from_secs(2)).unwrap();
        let async_value = asynchronous
            .execute(JiraOperation::Issue {
                key: "RUNI-1".into(),
                fields: None,
            })
            .await
            .unwrap();
        let blocking_root = root.clone();
        let blocking_value = tokio::task::spawn_blocking(move || {
            JiraClient::new(&blocking_root, credentials, Duration::from_secs(2))
                .unwrap()
                .execute(JiraOperation::Issue {
                    key: "RUNI-1".into(),
                    fields: None,
                })
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(async_value, blocking_value);
        for request in [requests.recv().unwrap(), requests.recv().unwrap()] {
            assert!(request.starts_with("GET /rest/api/3/issue/RUNI-1 HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: basic ")
            );
        }
    }
}
