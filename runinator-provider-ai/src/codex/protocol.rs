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

pub(super) struct AppSession {
    sink: Option<Arc<dyn ProviderEventSink>>,
    controls: Option<mpsc::Receiver<ProviderTerminalControl>>,
    token: CancellationToken,
    started: Instant,
    timeout: Duration,
    events: usize,
    bytes: usize,
    stderr: String,
    result: Option<String>,
    usage: Value,
    pub(super) turn_id: Option<String>,
    next_id: i64,
    steering: BTreeMap<i64, String>,
}

impl AppSession {
    pub(super) fn new(
        sink: Option<Arc<dyn ProviderEventSink>>,
        timeout_secs: i64,
        token: CancellationToken,
    ) -> Self {
        let controls = sink.as_ref().and_then(|sink| sink.take_terminal_control());
        Self {
            sink,
            controls,
            token,
            started: Instant::now(),
            timeout: Duration::from_secs(timeout_secs.max(1) as u64),
            events: 0,
            bytes: 0,
            stderr: String::new(),
            result: None,
            usage: Value::Null,
            turn_id: None,
            next_id: 10,
            steering: BTreeMap::new(),
        }
    }

    pub(super) fn wait_response(
        &mut self,
        child: &mut std::process::Child,
        stdin: &mut ChildStdin,
        receiver: &mpsc::Receiver<Line>,
        id: i64,
    ) -> Result<Value, SendableError> {
        loop {
            let event = self.next_event(child, stdin, receiver)?;
            if event.get("id").and_then(Value::as_i64) == Some(id) {
                if let Some(error) = event.get("error") {
                    return Err(CODEX_PROTOCOL.error(format!("Codex request failed: {error}")));
                }
                return Ok(event);
            }
            self.handle_event(event, stdin, None)?;
        }
    }

    pub(super) fn run_turn(
        &mut self,
        child: &mut std::process::Child,
        stdin: &mut ChildStdin,
        receiver: &mpsc::Receiver<Line>,
        thread_id: &str,
    ) -> Result<CompletedTurn, SendableError> {
        loop {
            self.drain_controls(stdin, thread_id)?;
            let event = self.next_event(child, stdin, receiver)?;
            if self.handle_event(event, stdin, Some(thread_id))? {
                return Ok(CompletedTurn {
                    result: self.result.clone().ok_or_else(|| {
                        CODEX_PROTOCOL.error("Codex turn completed without an agent message")
                    })?,
                    usage: self.usage.clone(),
                });
            }
        }
    }

    fn next_event(
        &mut self,
        child: &mut std::process::Child,
        _stdin: &mut ChildStdin,
        receiver: &mpsc::Receiver<Line>,
    ) -> Result<Value, SendableError> {
        loop {
            if self.token.is_cancelled() {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CODEX_CANCELED.bare());
            }
            if self.started.elapsed() >= self.timeout {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CODEX_TIMEOUT.error(format!(
                    "Codex timed out after {} seconds",
                    self.timeout.as_secs()
                )));
            }
            match receiver.recv_timeout(Duration::from_millis(20)) {
                Ok(line) if line.stream == "stderr" => {
                    append_bounded(&mut self.stderr, &line.content, MAX_STDERR_BYTES);
                    self.emit_chunk("stderr", line.content);
                }
                Ok(line) => {
                    if line.truncated {
                        return Err(
                            CODEX_PROTOCOL.error("Codex emitted an oversized JSON-RPC line")
                        );
                    }
                    self.events += 1;
                    self.bytes = self.bytes.saturating_add(line.content.len());
                    if self.events > MAX_EVENTS || self.bytes > MAX_EVENT_BYTES {
                        return Err(
                            CODEX_PROTOCOL.error("Codex app-server exceeded its event limits")
                        );
                    }
                    return serde_json::from_str(&line.content).map_err(|error| {
                        CODEX_PROTOCOL.error(format!("invalid Codex JSON-RPC message: {error}"))
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(status) = child
                        .try_wait()
                        .map_err(|error| Box::new(error) as SendableError)?
                    {
                        return Err(CODEX_EXIT_CODE.error(format!(
                            "Codex app-server exited with {status}: {}",
                            self.stderr
                        )));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CODEX_PROTOCOL.error("Codex app-server closed its output stream"));
                }
            }
        }
    }

    fn handle_event(
        &mut self,
        event: Value,
        stdin: &mut ChildStdin,
        thread_id: Option<&str>,
    ) -> Result<bool, SendableError> {
        if event.get("method").is_some() && event.get("id").is_some() {
            let id = event.get("id").cloned().unwrap_or(Value::Null);
            send_value(
                stdin,
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": { "code": -32000, "message": "Runinator denies interactive approvals" }
                }),
            )?;
            return Err(CODEX_PROTOCOL.error("Codex requested an approval or interactive input"));
        }
        if let Some(id) = event.get("id").and_then(Value::as_i64)
            && let Some(message) = self.steering.remove(&id)
        {
            if let Some(error) = event.get("error") {
                self.emit_progress(
                    "codex.steering_rejected",
                    json!({ "message": message, "error": error }),
                );
            } else {
                self.emit_progress("codex.steering", json!({ "message": message }));
            }
            return Ok(false);
        }
        let Some(method) = event.get("method").and_then(Value::as_str) else {
            return Ok(false);
        };
        let params = event.get("params").cloned().unwrap_or(Value::Null);
        self.emit_progress(
            &format!("codex.{}", method.replace('/', ".")),
            params.clone(),
        );
        if method == "item/completed"
            && let Some(text) = agent_message_text(params.get("item"))
        {
            self.result = Some(text);
        }
        if method == "thread/tokenUsage/updated" {
            self.usage = params.get("tokenUsage").cloned().unwrap_or(Value::Null);
        }
        if method == "turn/completed" {
            let status = params
                .pointer("/turn/status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            if status != "completed" {
                return Err(CODEX_EXIT_CODE.error(format!("Codex turn ended with status {status}")));
            }
            return Ok(true);
        }
        if method == "turn/started" {
            self.turn_id = params
                .pointer("/turn/id")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        if method == "turn/inputRequested"
            && let Some(thread_id) = thread_id
        {
            self.drain_controls(stdin, thread_id)?;
        }
        Ok(false)
    }

    fn drain_controls(
        &mut self,
        stdin: &mut ChildStdin,
        thread_id: &str,
    ) -> Result<(), SendableError> {
        let Some(receiver) = self.controls.as_ref() else {
            return Ok(());
        };
        loop {
            match receiver.try_recv() {
                Ok(ProviderTerminalControl::Input { data }) => {
                    let turn_id = self.turn_id.as_deref().ok_or_else(|| {
                        CODEX_INPUT.error("Codex turn has not started and cannot accept steering")
                    })?;
                    let id = self.next_id;
                    self.next_id += 1;
                    send_request(
                        stdin,
                        id,
                        "turn/steer",
                        json!({
                            "threadId": thread_id,
                            "expectedTurnId": turn_id,
                            "input": [{ "type": "text", "text": data }],
                        }),
                    )?;
                    self.steering.insert(id, data);
                }
                Ok(ProviderTerminalControl::Eof) => {
                    self.controls = None;
                    return Ok(());
                }
                Ok(ProviderTerminalControl::Resize { cols, rows }) => {
                    self.emit_progress(
                        "codex.resize_ignored",
                        json!({ "cols": cols, "rows": rows }),
                    );
                }
                Err(mpsc::TryRecvError::Empty) | Err(mpsc::TryRecvError::Disconnected) => {
                    return Ok(());
                }
            }
        }
    }

    fn emit_progress(&self, kind: &str, payload: Value) {
        if let Some(sink) = &self.sink {
            sink.emit(ProviderExecutionEvent::Progress {
                kind: kind.into(),
                payload,
            });
        }
    }

    fn emit_chunk(&self, stream: &str, content: String) {
        if let Some(sink) = &self.sink {
            sink.emit(ProviderExecutionEvent::Chunk {
                stream: stream.into(),
                content,
            });
        }
    }
}

pub(super) struct Line {
    stream: &'static str,
    content: String,
    truncated: bool,
}

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
