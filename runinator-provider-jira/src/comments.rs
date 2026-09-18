use std::fs;
use std::path::Path;

use runinator_jira::{JiraClient, JiraOperation};
use runinator_models::{
    errors::SendableError,
    runs::{NewRunArtifact, TaskExecutionResult},
};
use runinator_provider_support::polling::{PollPage, poll_pages};
use serde_json::{Value, json};

use crate::error::{IO_ERROR, client_error};
use crate::params::JiraCommentsParams;

// fetches an issue's comments, renders each comment body (atlassian document
// format) to plain text the way an llm wants it, and downloads any image
// attachments so a downstream ai step can read them. returns parsed text plus
// image file references, and registers the images as run artifacts.
pub(crate) fn jira_fetch_comments(
    client: &JiraClient,
    p: &JiraCommentsParams,
    artifact_dir: &str,
) -> Result<TaskExecutionResult, SendableError> {
    let comments = fetch_all_comments(client, &p.key)?;
    let mut rendered: Vec<Value> = Vec::with_capacity(comments.len());
    let mut text_blocks: Vec<String> = Vec::with_capacity(comments.len());
    for comment in &comments {
        let author = comment
            .get("author")
            .and_then(|a| a.get("displayName"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let created = comment
            .get("created")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let id = comment
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let body_text = render_comment_body(comment.get("body"));
        text_blocks.push(format!("[{author} — {created}]\n{body_text}"));
        rendered.push(json!({
            "id": id,
            "author": author,
            "created": created,
            "text": body_text,
        }));
    }

    let attachments = fetch_image_attachments(client, &p.key)?;
    let target_dir = p.download_dir.as_deref().unwrap_or(artifact_dir);
    if !target_dir.is_empty() {
        fs::create_dir_all(target_dir)
            .map_err(|e| IO_ERROR.error(format!("could not create {target_dir}: {e}")))?;
    }

    let mut images: Vec<Value> = Vec::new();
    let mut artifacts: Vec<NewRunArtifact> = Vec::new();
    for attachment in &attachments {
        let content_url = attachment
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let filename = attachment
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or("image");
        let mime_type = attachment
            .get("mimeType")
            .and_then(Value::as_str)
            .unwrap_or("application/octet-stream");
        let att_id = attachment
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if content_url.is_empty() {
            continue;
        }
        let bytes = client
            .download(content_url.to_owned())
            .map_err(|error| client_error("jira attachment download failed", error))?;
        let safe_name = sanitize_filename(filename);
        let stem = if att_id.is_empty() {
            safe_name.clone()
        } else {
            format!("{att_id}-{safe_name}")
        };
        let mut path = String::new();
        if !target_dir.is_empty() {
            let dest = Path::new(target_dir).join(&stem);
            fs::write(&dest, &bytes)
                .map_err(|e| IO_ERROR.error(format!("could not write {}: {e}", dest.display())))?;
            path = dest.to_string_lossy().into_owned();
            artifacts.push(NewRunArtifact {
                name: stem.clone(),
                mime_type: mime_type.to_string(),
                size_bytes: bytes.len() as i64,
                uri: path.clone(),
                metadata: json!({ "provider": "JIRA", "issue": p.key, "filename": filename })
                    .into(),
            });
        }
        images.push(json!({
            "attachment_id": attachment.get("id").cloned().unwrap_or(Value::Null),
            "filename": filename,
            "mime_type": mime_type,
            "source_url": content_url,
            "path": path,
            "size_bytes": bytes.len(),
        }));
    }

    let output = json!({
        "key": p.key,
        "comment_count": rendered.len(),
        "image_count": images.len(),
        "text": text_blocks.join("\n\n---\n\n"),
        "comments": rendered,
        "images": images,
    });
    Ok(TaskExecutionResult {
        message: Some(format!(
            "fetched {} jira comment(s) and {} image(s)",
            rendered.len(),
            images.len()
        )),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts,
    })
}

// pages through the comment endpoint until every comment is collected.
pub(crate) fn fetch_all_comments(
    client: &JiraClient,
    key: &str,
) -> Result<Vec<Value>, SendableError> {
    poll_pages(
        Some(0u64),
        100,
        |start_at| {
            let start_at = start_at.unwrap_or_default();
            let value = client
                .execute(JiraOperation::Comments {
                    key: key.to_owned(),
                    start_at,
                    max_results: 100,
                })
                .map_err(|error| client_error("jira comments request failed", error))?;
            let items = value
                .get("comments")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            let total = value
                .get("total")
                .and_then(Value::as_u64)
                .unwrap_or(start_at + items.len() as u64);
            let next = start_at + items.len() as u64;
            let next_cursor = (!items.is_empty() && next < total).then_some(next);
            Ok::<_, SendableError>(PollPage { items, next_cursor })
        },
        |limit| {
            crate::error::HTTP_ERROR.error(format!("jira comment pagination failed: {limit:?}"))
        },
    )
}

// reads the issue's attachment list and keeps the image/* ones.
fn fetch_image_attachments(client: &JiraClient, key: &str) -> Result<Vec<Value>, SendableError> {
    let value = client
        .execute(JiraOperation::Issue {
            key: key.to_owned(),
            fields: Some("attachment".into()),
        })
        .map_err(|error| client_error("jira attachment request failed", error))?;
    let attachments = value
        .get("fields")
        .and_then(|f| f.get("attachment"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(attachments
        .into_iter()
        .filter(|a| {
            a.get("mimeType")
                .and_then(Value::as_str)
                .map(|m| m.starts_with("image/"))
                .unwrap_or(false)
        })
        .collect())
}

// keeps a downloaded filename safe to write: strips path separators and control
// characters, falling back to a generic name.
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('_');
    if trimmed.is_empty() {
        "image".to_string()
    } else {
        trimmed.to_string()
    }
}

// renders a comment body into plain text. handles both the modern atlassian
// document format (a node tree) and the legacy plain-string body.
pub(crate) fn render_comment_body(body: Option<&Value>) -> String {
    runinator_jira::render_comment_body(body)
}
