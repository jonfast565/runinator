use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Read, Write},
    process::ChildStdin,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use runinator_models::{
    errors::SendableError,
    json,
    runs::{ProviderExecutionEvent, ProviderTerminalControl},
    value::Value,
};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};

use super::{CompletedTurn, MAX_EVENT_BYTES, MAX_EVENTS, MAX_LINE_BYTES, MAX_STDERR_BYTES};
use crate::errors::{CODEX_CANCELED, CODEX_EXIT_CODE, CODEX_INPUT, CODEX_PROTOCOL, CODEX_TIMEOUT};

pub(super) fn spawn_reader<R: Read + Send + 'static>(
    reader: R,
    stream: &'static str,
    sender: mpsc::SyncSender<Line>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        while let Ok(Some((content, truncated))) = read_bounded_line(&mut reader, MAX_LINE_BYTES) {
            if sender
                .send(Line {
                    stream,
                    content,
                    truncated,
                })
                .is_err()
            {
                break;
            }
        }
    })
}

fn read_bounded_line<R: BufRead>(
    reader: &mut R,
    max: usize,
) -> std::io::Result<Option<(String, bool)>> {
    let mut output = Vec::new();
    let mut seen = false;
    let mut truncated = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return if seen {
                Ok(Some((
                    String::from_utf8_lossy(&output).into_owned(),
                    truncated,
                )))
            } else {
                Ok(None)
            };
        }
        seen = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let content = newline.unwrap_or(available.len());
        let copied = (max - output.len()).min(content);
        output.extend_from_slice(&available[..copied]);
        truncated |= copied < content;
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }
    Ok(Some((
        String::from_utf8_lossy(&output)
            .trim_end_matches('\r')
            .into(),
        truncated,
    )))
}

pub(super) fn send_request(
    stdin: &mut ChildStdin,
    id: i64,
    method: &str,
    params: Value,
) -> Result<(), SendableError> {
    send_value(
        stdin,
        json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }),
    )
}

pub(super) fn send_notification(
    stdin: &mut ChildStdin,
    method: &str,
    params: Value,
) -> Result<(), SendableError> {
    send_value(
        stdin,
        json!({ "jsonrpc": "2.0", "method": method, "params": params }),
    )
}

fn send_value(stdin: &mut ChildStdin, value: Value) -> Result<(), SendableError> {
    serde_json::to_writer(&mut *stdin, &value).map_err(|error| CODEX_PROTOCOL.error(error))?;
    stdin
        .write_all(b"\n")
        .and_then(|()| stdin.flush())
        .map_err(|error| CODEX_PROTOCOL.error(format!("could not write Codex request: {error}")))
}

pub(super) fn agent_message_text(item: Option<&Value>) -> Option<String> {
    let item = item?;
    let kind = item.get("type").and_then(Value::as_str)?;
    if !matches!(kind, "agent_message" | "agentMessage") {
        return None;
    }
    item.get("text")
        .or_else(|| item.get("content"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn append_bounded(output: &mut String, value: &str, max: usize) {
    let remaining = max.saturating_sub(output.len());
    let mut copied = remaining.min(value.len());
    while !value.is_char_boundary(copied) {
        copied -= 1;
    }
    output.push_str(&value[..copied]);
}

mod app_session;
pub(super) use app_session::AppSession;

mod line;
pub(super) use line::Line;
