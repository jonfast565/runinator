//! Shared typed GitHub API clients.

pub mod errors;

mod async_git_hub_client;
mod git_hub_client;
mod request_spec;

use std::{process::Stdio, time::Duration};

use reqwest::{Method, Url};
use serde_json::{Value, json};
use tokio::{process::Command, time::timeout};

pub use async_git_hub_client::AsyncGitHubClient;
pub use git_hub_client::GitHubClient;

use errors::GitHubError;
use request_spec::RequestSpec;

const API_ROOT: &str = "https://api.github.com";
const ACCEPT: &str = "application/vnd.github+json";

#[derive(Clone)]
enum GitHubTransport {
    Http {
        client: reqwest::Client,
        token: String,
        api_root: Url,
    },
    Cli {
        token: Option<String>,
        timeout: Duration,
        max_output_bytes: usize,
    },
}

pub enum GitHubOperation {
    Repository {
        repository: String,
    },
    PullRequests {
        repository: String,
        state: String,
        head: Option<String>,
        per_page: u32,
        page: u32,
    },
    CreatePull {
        repository: String,
        title: String,
        head: String,
        base: String,
        body: String,
    },
    UpdatePull {
        repository: String,
        number: i64,
        title: String,
        base: String,
        body: String,
    },
    Reviews {
        repository: String,
        pull_number: String,
    },
    MergePull {
        repository: String,
        pull_number: String,
        body: Value,
    },
    IssueComments {
        repository: String,
        issue_number: String,
        per_page: u32,
        page: u32,
    },
    /// Conversation comments across every issue and pull request in a repository, newest first.
    /// Polling needs one repository-wide call; the per-issue listing would cost one call per pull
    /// request and exhaust the hourly quota on a busy repository.
    RepositoryIssueComments {
        repository: String,
        per_page: u32,
        page: u32,
    },
    AddComment {
        repository: String,
        issue_number: String,
        body: String,
    },
    RequestReviewers {
        repository: String,
        pull_number: String,
        reviewers: Vec<String>,
        team_reviewers: Vec<String>,
    },
    AddAssignees {
        repository: String,
        issue_number: String,
        assignees: Vec<String>,
    },
    CheckRuns {
        repository: String,
        git_ref: String,
        per_page: Option<u32>,
        page: Option<u32>,
    },
    DispatchWorkflow {
        repository: String,
        workflow_id: String,
        git_ref: String,
        inputs: Value,
    },
    WorkflowRuns {
        repository: String,
        workflow_id: Option<String>,
        branch: Option<String>,
        event: Option<String>,
        status: Option<String>,
        per_page: Option<u32>,
        page: Option<u32>,
    },
    RerunWorkflow {
        repository: String,
        run_id: String,
    },
    RerequestCheck {
        repository: String,
        check_run_id: String,
    },
    Commits {
        repository: String,
        since: Option<String>,
        per_page: u32,
        page: u32,
    },
}

impl GitHubOperation {
    /// Whether the operation's request carries a `page` parameter, and so can be walked page by
    /// page. An operation that ignores `page` returns the same body for every page number, which a
    /// generic page walk reads as a full page and keeps requesting: identical items repeat until
    /// the page budget trips, and the budget surfaces as a hard error that retains the checkpoint.
    /// Declaring it here keeps the answer with the request that decides it.
    pub fn paginates(&self) -> bool {
        use GitHubOperation::*;
        matches!(
            self,
            PullRequests { .. }
                | IssueComments { .. }
                | RepositoryIssueComments { .. }
                | CheckRuns { .. }
                | WorkflowRuns { .. }
                | Commits { .. }
        )
    }

    /// The operation's variant name, for diagnostics that have to say which call was refused.
    pub fn name(&self) -> &'static str {
        use GitHubOperation::*;
        match self {
            Repository { .. } => "Repository",
            PullRequests { .. } => "PullRequests",
            CreatePull { .. } => "CreatePull",
            UpdatePull { .. } => "UpdatePull",
            Reviews { .. } => "Reviews",
            MergePull { .. } => "MergePull",
            IssueComments { .. } => "IssueComments",
            RepositoryIssueComments { .. } => "RepositoryIssueComments",
            AddComment { .. } => "AddComment",
            RequestReviewers { .. } => "RequestReviewers",
            AddAssignees { .. } => "AddAssignees",
            CheckRuns { .. } => "CheckRuns",
            DispatchWorkflow { .. } => "DispatchWorkflow",
            WorkflowRuns { .. } => "WorkflowRuns",
            RerunWorkflow { .. } => "RerunWorkflow",
            RerequestCheck { .. } => "RerequestCheck",
            Commits { .. } => "Commits",
        }
    }

    fn request(self) -> RequestSpec {
        use GitHubOperation::*;
        match self {
            Repository { repository } => get(format!("/repos/{repository}")),
            PullRequests {
                repository,
                state,
                head,
                per_page,
                page,
            } => {
                let mut request = get(format!("/repos/{repository}/pulls"));
                request.query.extend([
                    ("state".into(), state),
                    ("sort".into(), "updated".into()),
                    ("direction".into(), "desc".into()),
                    ("per_page".into(), per_page.to_string()),
                    ("page".into(), page.to_string()),
                ]);
                if let Some(head) = head {
                    request.query.push(("head".into(), head));
                }
                request
            }
            CreatePull {
                repository,
                title,
                head,
                base,
                body,
            } => post(
                format!("/repos/{repository}/pulls"),
                json!({ "title": title, "head": head, "base": base, "body": body }),
            ),
            UpdatePull {
                repository,
                number,
                title,
                base,
                body,
            } => request(
                Method::PATCH,
                format!("/repos/{repository}/pulls/{number}"),
                Some(json!({ "title": title, "base": base, "body": body })),
            ),
            Reviews {
                repository,
                pull_number,
            } => get(format!("/repos/{repository}/pulls/{pull_number}/reviews")),
            MergePull {
                repository,
                pull_number,
                body,
            } => request(
                Method::PUT,
                format!("/repos/{repository}/pulls/{pull_number}/merge"),
                Some(body),
            ),
            IssueComments {
                repository,
                issue_number,
                per_page,
                page,
            } => {
                let mut request = get(format!(
                    "/repos/{repository}/issues/{issue_number}/comments"
                ));
                request.query.extend([
                    ("per_page".into(), per_page.to_string()),
                    ("page".into(), page.to_string()),
                ]);
                request
            }
            RepositoryIssueComments {
                repository,
                per_page,
                page,
            } => {
                let mut request = get(format!("/repos/{repository}/issues/comments"));
                request.query.extend([
                    ("sort".into(), "updated".into()),
                    ("direction".into(), "desc".into()),
                    ("per_page".into(), per_page.to_string()),
                    ("page".into(), page.to_string()),
                ]);
                request
            }
            AddComment {
                repository,
                issue_number,
                body,
            } => post(
                format!("/repos/{repository}/issues/{issue_number}/comments"),
                json!({ "body": body }),
            ),
            RequestReviewers {
                repository,
                pull_number,
                reviewers,
                team_reviewers,
            } => post(
                format!("/repos/{repository}/pulls/{pull_number}/requested_reviewers"),
                json!({ "reviewers": reviewers, "team_reviewers": team_reviewers }),
            ),
            AddAssignees {
                repository,
                issue_number,
                assignees,
            } => post(
                format!("/repos/{repository}/issues/{issue_number}/assignees"),
                json!({ "assignees": assignees }),
            ),
            CheckRuns {
                repository,
                git_ref,
                per_page,
                page,
            } => {
                let mut request = get(format!("/repos/{repository}/commits/{git_ref}/check-runs"));
                if let Some(value) = per_page {
                    request.query.push(("per_page".into(), value.to_string()));
                }
                if let Some(value) = page {
                    request.query.push(("page".into(), value.to_string()));
                }
                request
            }
            DispatchWorkflow {
                repository,
                workflow_id,
                git_ref,
                inputs,
            } => post(
                format!("/repos/{repository}/actions/workflows/{workflow_id}/dispatches"),
                json!({ "ref": git_ref, "inputs": inputs }),
            ),
            WorkflowRuns {
                repository,
                workflow_id,
                branch,
                event,
                status,
                per_page,
                page,
            } => {
                let path = workflow_id.map_or_else(
                    || format!("/repos/{repository}/actions/runs"),
                    |workflow| format!("/repos/{repository}/actions/workflows/{workflow}/runs"),
                );
                let mut request = get(path);
                for (name, value) in [("branch", branch), ("event", event), ("status", status)] {
                    if let Some(value) = value {
                        request.query.push((name.into(), value));
                    }
                }
                if let Some(value) = per_page {
                    request.query.push(("per_page".into(), value.to_string()));
                }
                if let Some(value) = page {
                    request.query.push(("page".into(), value.to_string()));
                }
                request
            }
            RerunWorkflow { repository, run_id } => post(
                format!("/repos/{repository}/actions/runs/{run_id}/rerun"),
                json!({}),
            ),
            RerequestCheck {
                repository,
                check_run_id,
            } => post(
                format!("/repos/{repository}/check-runs/{check_run_id}/rerequest"),
                json!({}),
            ),
            Commits {
                repository,
                since,
                per_page,
                page,
            } => {
                let mut request = get(format!("/repos/{repository}/commits"));
                request.query.extend([
                    ("per_page".into(), per_page.to_string()),
                    ("page".into(), page.to_string()),
                ]);
                if let Some(since) = since {
                    request.query.push(("since".into(), since));
                }
                request
            }
        }
    }
}

fn request(method: Method, path: String, body: Option<Value>) -> RequestSpec {
    RequestSpec {
        method,
        path,
        query: Vec::new(),
        body,
    }
}

fn get(path: String) -> RequestSpec {
    request(Method::GET, path, None)
}
fn post(path: String, body: Value) -> RequestSpec {
    request(Method::POST, path, Some(body))
}

fn request_url(api_root: &Url, spec: &RequestSpec) -> Result<Url, GitHubError> {
    let mut url = api_root
        .join(spec.path.trim_start_matches('/'))
        .map_err(GitHubError::config)?;
    if !spec.query.is_empty() {
        url.query_pairs_mut().extend_pairs(spec.query.iter());
    }
    Ok(url)
}

async fn execute_http(
    client: &reqwest::Client,
    token: &str,
    api_root: &Url,
    spec: RequestSpec,
) -> Result<Value, GitHubError> {
    let url = request_url(api_root, &spec)?;
    let mut request = client
        .request(spec.method, url)
        .bearer_auth(token)
        .header("Accept", ACCEPT)
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(body) = spec.body {
        request = request.json(&body);
    }
    let response = request.send().await.map_err(GitHubError::request)?;
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse().ok());
    let status = response.status();
    let text = response.text().await.map_err(GitHubError::request)?;
    if !status.is_success() {
        return Err(GitHubError::status(status.as_u16(), text, retry_after));
    }
    decode(status.as_u16(), &text)
}

async fn execute_cli(
    token: Option<&str>,
    duration: Duration,
    max_output_bytes: usize,
    spec: RequestSpec,
) -> Result<Value, GitHubError> {
    let url = request_url(&Url::parse(API_ROOT).map_err(GitHubError::config)?, &spec)?;
    let endpoint = url.as_str().strip_prefix(API_ROOT).unwrap_or(url.as_str());
    let mut command = Command::new("gh");
    command
        .args([
            "api",
            "--method",
            spec.method.as_str(),
            "--header",
            &format!("Accept: {ACCEPT}"),
            "--header",
            "X-GitHub-Api-Version: 2022-11-28",
            endpoint,
        ])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("NO_COLOR", "1")
        .kill_on_drop(true)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(token) = token {
        command.env("GH_TOKEN", token);
    }
    if let Some(body) = spec.body {
        command.args(["--input", "-"]);
        command.stdin(Stdio::piped());
        let mut child = command.spawn().map_err(GitHubError::command)?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin
                .write_all(&serde_json::to_vec(&body).map_err(GitHubError::json)?)
                .await
                .map_err(GitHubError::command)?;
        }
        let output = timeout(duration, child.wait_with_output())
            .await
            .map_err(|_| GitHubError::timeout())?
            .map_err(GitHubError::command)?;
        return decode_cli_output(output, max_output_bytes);
    }
    let output = timeout(duration, command.output())
        .await
        .map_err(|_| GitHubError::timeout())?
        .map_err(GitHubError::command)?;
    decode_cli_output(output, max_output_bytes)
}

fn decode_cli_output(
    output: std::process::Output,
    max_output_bytes: usize,
) -> Result<Value, GitHubError> {
    if output.stdout.len().saturating_add(output.stderr.len()) > max_output_bytes {
        return Err(GitHubError::output_too_large(max_output_bytes));
    }
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if message.to_ascii_lowercase().contains("rate limit") {
            GitHubError::status(429, message, None)
        } else {
            GitHubError::command(message)
        });
    }
    decode(200, &String::from_utf8_lossy(&output.stdout))
}

fn decode(status: u16, text: &str) -> Result<Value, GitHubError> {
    if text.trim().is_empty() {
        return Ok(json!({ "status": status }));
    }
    serde_json::from_str(text).map_err(GitHubError::json)
}

#[cfg(test)]
mod tests {
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
                let body = r#"{"id":7}"#;
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
        });
        (format!("http://{address}/"), receiver)
    }

    #[test]
    fn operations_own_github_endpoint_construction() {
        let spec = GitHubOperation::CheckRuns {
            repository: "octo/example".into(),
            git_ref: "abc".into(),
            per_page: Some(100),
            page: Some(2),
        }
        .request();
        let url = request_url(&Url::parse(API_ROOT).unwrap(), &spec).unwrap();
        assert_eq!(url.path(), "/repos/octo/example/commits/abc/check-runs");
        assert_eq!(url.query(), Some("per_page=100&page=2"));
    }

    #[test]
    fn consumers_do_not_construct_github_calls_directly() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        for relative in [
            "runinator-provider-github/src/lib.rs",
            "runinator-adapter-host/src/main.rs",
        ] {
            let source = std::fs::read_to_string(root.join(relative)).unwrap();
            assert!(
                !source.contains("api.github.com"),
                "{relative} constructs a GitHub URL"
            );
            assert!(
                !source.contains("reqwest::"),
                "{relative} calls GitHub through reqwest"
            );
            assert!(
                !source.contains("Command::new(\"gh\")"),
                "{relative} spawns gh directly"
            );
        }
    }

    #[tokio::test]
    async fn blocking_and_async_clients_share_operation_behavior() {
        let (root, requests) = server(2);
        let asynchronous =
            AsyncGitHubClient::http_at(&root, "secret", Duration::from_secs(2)).unwrap();
        let async_value = asynchronous
            .execute(GitHubOperation::Repository {
                repository: "octo/example".into(),
            })
            .await
            .unwrap();
        let blocking_root = root.clone();
        let blocking_value = tokio::task::spawn_blocking(move || {
            GitHubClient::http_at(&blocking_root, "secret", Duration::from_secs(2))
                .unwrap()
                .execute(GitHubOperation::Repository {
                    repository: "octo/example".into(),
                })
                .unwrap()
        })
        .await
        .unwrap();
        assert_eq!(async_value, blocking_value);
        for request in [requests.recv().unwrap(), requests.recv().unwrap()] {
            assert!(
                request.starts_with("GET /repos/octo/example HTTP/1.1"),
                "unexpected request: {request}"
            );
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer secret")
            );
        }
    }
}
