use std::fs;
use std::path::Path;

use runinator_models::{
    errors::SendableError,
    runs::{NewRunArtifact, TaskExecutionResult},
};
use serde_json::{Value, json};

use crate::error::{IO_ERROR, http_error, validate_base_url};
use crate::params::JiraCommentsParams;
use crate::response::http_status_error;

// fetches an issue's comments, renders each comment body (atlassian document
// format) to plain text the way an llm wants it, and downloads any image
// attachments so a downstream ai step can read them. returns parsed text plus
// image file references, and registers the images as run artifacts.
pub(crate) fn jira_fetch_comments(
    client: &reqwest::blocking::Client,
    p: &JiraCommentsParams,
    artifact_dir: &str,
) -> Result<TaskExecutionResult, SendableError> {
    validate_base_url(&p.base.base_url)?;
    let base = p.base.base_url.trim_end_matches('/');
    let auth_user = p.base.email.as_deref().unwrap_or_default();

    let comments = fetch_all_comments(client, base, auth_user, &p.base.token, &p.key)?;
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

    let attachments = fetch_image_attachments(client, base, auth_user, &p.base.token, &p.key)?;
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
        let bytes = download_bytes(client, content_url, auth_user, &p.base.token)?;
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
fn fetch_all_comments(
    client: &reqwest::blocking::Client,
    base: &str,
    auth_user: &str,
    token: &str,
    key: &str,
) -> Result<Vec<Value>, SendableError> {
    let mut all = Vec::new();
    let mut start_at = 0i64;
    loop {
        let url = format!("{base}/rest/api/3/issue/{key}/comment");
        let response = client
            .get(&url)
            .basic_auth(auth_user, Some(token))
            .query(&[
                ("startAt", start_at.to_string()),
                ("maxResults", "100".to_string()),
            ])
            .send()
            .map_err(|e| http_error("jira comments request failed", e))?;
        let value = read_json(response)?;
        let page: Vec<Value> = value
            .get("comments")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let page_len = page.len() as i64;
        all.extend(page);
        let total = value
            .get("total")
            .and_then(Value::as_i64)
            .unwrap_or(all.len() as i64);
        start_at += page_len;
        if page_len == 0 || start_at >= total {
            break;
        }
    }
    Ok(all)
}

// reads the issue's attachment list and keeps the image/* ones.
fn fetch_image_attachments(
    client: &reqwest::blocking::Client,
    base: &str,
    auth_user: &str,
    token: &str,
    key: &str,
) -> Result<Vec<Value>, SendableError> {
    let url = format!("{base}/rest/api/3/issue/{key}");
    let response = client
        .get(&url)
        .basic_auth(auth_user, Some(token))
        .query(&[("fields", "attachment")])
        .send()
        .map_err(|e| http_error("jira attachment request failed", e))?;
    let value = read_json(response)?;
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

fn download_bytes(
    client: &reqwest::blocking::Client,
    url: &str,
    auth_user: &str,
    token: &str,
) -> Result<Vec<u8>, SendableError> {
    let response = client
        .get(url)
        .basic_auth(auth_user, Some(token))
        .send()
        .map_err(|e| http_error("jira attachment download failed", e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(http_status_error(
            status,
            &response.text().unwrap_or_default(),
        ));
    }
    let bytes = response
        .bytes()
        .map_err(|e| http_error("jira attachment read failed", e))?;
    Ok(bytes.to_vec())
}

fn read_json(response: reqwest::blocking::Response) -> Result<Value, SendableError> {
    let status = response.status();
    let text = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(http_status_error(status, &text));
    }
    Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
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
    match body {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(node @ Value::Object(_)) => {
            let mut out = String::new();
            walk_adf(node, &mut out);
            collapse_blank_lines(&out)
        }
        _ => String::new(),
    }
}

// walks an adf node, appending text and image placeholders; block-level nodes are
// separated by newlines so the rendered text reads naturally.
fn walk_adf(node: &Value, out: &mut String) {
    let node_type = node.get("type").and_then(Value::as_str).unwrap_or("");
    match node_type {
        "text" => {
            if let Some(s) = node.get("text").and_then(Value::as_str) {
                out.push_str(s);
            }
            return;
        }
        "hardBreak" => {
            out.push('\n');
            return;
        }
        "media" | "mediaInline" => {
            let attrs = node.get("attrs");
            let alt = attrs
                .and_then(|a| a.get("alt"))
                .and_then(Value::as_str)
                .or_else(|| {
                    attrs
                        .and_then(|a| a.get("__fileName"))
                        .and_then(Value::as_str)
                })
                .unwrap_or("image");
            out.push_str(&format!("[image: {alt}]"));
            return;
        }
        "mention" => {
            if let Some(text) = node
                .get("attrs")
                .and_then(|a| a.get("text"))
                .and_then(Value::as_str)
            {
                out.push_str(text);
            }
            return;
        }
        "listItem" => out.push_str("- "),
        _ => {}
    }

    if let Some(content) = node.get("content").and_then(Value::as_array) {
        for child in content {
            walk_adf(child, out);
        }
    }

    if matches!(
        node_type,
        "paragraph" | "heading" | "listItem" | "blockquote" | "codeBlock" | "rule"
    ) {
        out.push('\n');
    }
}

// squeezes runs of 3+ newlines down to a paragraph break and trims edges.
fn collapse_blank_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut newline_run = 0;
    for ch in text.chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                result.push('\n');
            }
        } else {
            newline_run = 0;
            result.push(ch);
        }
    }
    result.trim().to_string()
}
