#[allow(unused_imports)]
use super::*;

pub(crate) struct AppSession {
    pub(super) sink: Option<Arc<dyn ProviderEventSink>>,
    pub(super) controls: Option<mpsc::Receiver<ProviderTerminalControl>>,
    pub(super) token: CancellationToken,
    pub(super) started: Instant,
    pub(super) timeout: Duration,
    pub(super) events: usize,
    pub(super) bytes: usize,
    pub(super) stderr: String,
    pub(super) result: Option<String>,
    pub(super) usage: Value,
    pub(crate) turn_id: Option<String>,
    pub(super) next_id: i64,
    pub(super) steering: BTreeMap<i64, String>,
}

impl AppSession {
    pub(crate) fn new(
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

    pub(crate) fn wait_response(
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

    pub(crate) fn run_turn(
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

    pub(super) fn next_event(
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

    pub(super) fn handle_event(
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

    pub(super) fn drain_controls(
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

    pub(super) fn emit_progress(&self, kind: &str, payload: Value) {
        if let Some(sink) = &self.sink {
            sink.emit(ProviderExecutionEvent::Progress {
                kind: kind.into(),
                payload,
            });
        }
    }

    pub(super) fn emit_chunk(&self, stream: &str, content: String) {
        if let Some(sink) = &self.sink {
            sink.emit(ProviderExecutionEvent::Chunk {
                stream: stream.into(),
                content,
            });
        }
    }
}
