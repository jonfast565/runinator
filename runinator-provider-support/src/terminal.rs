//! Cross-platform PTY/ConPTY execution with Runinator terminal controls.

use std::{
    fmt,
    io::{Read, Write},
    sync::{Arc, mpsc::Receiver},
    thread,
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
pub use portable_pty::CommandBuilder;
use portable_pty::{PtySize, native_pty_system};
use runinator_models::runs::{
    ProviderExecutionEvent, ProviderTerminalControl, TerminalInteraction, TerminalInteractionState,
};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};
use serde::{Deserialize, Serialize};

pub const ALLOW_INTERACTIVE_ENV: &str = "RUNINATOR_CONSOLE_ALLOW_INTERACTIVE";
const OSC_PREFIX: &[u8] = b"\x1b]777;runinator;";
const MAX_PROTOCOL_PAYLOAD: usize = 16 * 1024;
const MAX_REQUEST_ID: usize = 128;
const MAX_PROMPT: usize = 8 * 1024;

pub fn interactive_permitted() -> bool {
    matches!(std::env::var(ALLOW_INTERACTIVE_ENV).ok().as_deref(), Some(value) if !value.is_empty() && value != "0")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalExit {
    pub success: bool,
    pub exit_code: i32,
}

#[derive(Debug)]
pub enum TerminalError {
    Canceled,
    TimedOut(Duration),
    Unavailable(String),
}

impl fmt::Display for TerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => write!(formatter, "terminal process canceled"),
            Self::TimedOut(timeout) => {
                write!(
                    formatter,
                    "terminal process timed out after {} seconds",
                    timeout.as_secs()
                )
            }
            Self::Unavailable(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TerminalError {}

/// Run `command` in the platform's native pseudo-terminal. Output is emitted as raw `terminal`
/// chunks so an ANSI terminal emulator can reproduce cursor movement and color exactly.
pub fn run(
    command: CommandBuilder,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
    timeout: Duration,
) -> Result<TerminalExit, TerminalError> {
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize::default()).map_err(unavailable)?;
    let mut child = pair.slave.spawn_command(command).map_err(unavailable)?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().map_err(unavailable)?;
    let output = spawn_reader(reader, sink.clone());
    let controls = sink
        .as_ref()
        .and_then(|sink| sink.take_terminal_control())
        .unwrap_or_else(disconnected_receiver);
    let mut writer = Some(pair.master.take_writer().map_err(unavailable)?);
    let started = Instant::now();

    let status = loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break Err(TerminalError::Canceled);
        }
        while let Ok(control) = controls.try_recv() {
            apply_control(pair.master.as_ref(), &mut writer, control)?;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TerminalError::TimedOut(timeout));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TerminalError::Unavailable(error.to_string()));
            }
        }
    };
    drop(writer);
    let _ = output.join();
    let status = status?;
    Ok(TerminalExit {
        success: status.success(),
        exit_code: i32::try_from(status.exit_code()).unwrap_or(-1),
    })
}

fn apply_control(
    master: &dyn portable_pty::MasterPty,
    writer: &mut Option<Box<dyn Write + Send>>,
    control: ProviderTerminalControl,
) -> Result<(), TerminalError> {
    match control {
        ProviderTerminalControl::Input { data } => {
            if let Some(writer) = writer.as_mut() {
                writer
                    .write_all(data.as_bytes())
                    .and_then(|()| writer.flush())
                    .map_err(|error| TerminalError::Unavailable(error.to_string()))?;
            }
        }
        ProviderTerminalControl::Resize { cols, rows } => master
            .resize(PtySize {
                cols: cols.max(1),
                rows: rows.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(unavailable)?,
        ProviderTerminalControl::Eof => {
            // A PTY is one bidirectional terminal, so dropping only our writer clone does not close
            // the master while the output reader is alive. Send the terminal EOT byte instead;
            // line disciplines and interactive CLIs interpret it exactly like Ctrl+D.
            if let Some(writer) = writer.as_mut() {
                writer
                    .write_all(&[0x04])
                    .and_then(|()| writer.flush())
                    .map_err(|error| TerminalError::Unavailable(error.to_string()))?;
            }
        }
    }
    Ok(())
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        let mut protocol = TerminalProtocolParser::default();
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if let Some(sink) = &sink {
                        for event in protocol.push(&buffer[..read]) {
                            sink.emit(event);
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        if let Some(sink) = &sink {
            for event in protocol.finish() {
                sink.emit(event);
            }
        }
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct TerminalProtocolPayload {
    version: u8,
    event: String,
    request_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    prompt: Option<String>,
}

/// Encode the portable marker a program writes immediately before it blocks for terminal input.
pub fn input_required_marker(request_id: &str, prompt: &str) -> Result<String, serde_json::Error> {
    encode_marker(TerminalProtocolPayload {
        version: 1,
        event: "input_required".into(),
        request_id: request_id.into(),
        prompt: Some(prompt.into()),
    })
}

/// Encode the matching marker a program writes only after it has accepted the submitted input.
pub fn input_accepted_marker(request_id: &str) -> Result<String, serde_json::Error> {
    encode_marker(TerminalProtocolPayload {
        version: 1,
        event: "input_accepted".into(),
        request_id: request_id.into(),
        prompt: None,
    })
}

fn encode_marker(payload: TerminalProtocolPayload) -> Result<String, serde_json::Error> {
    let encoded = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?);
    Ok(format!("\x1b]777;runinator;{encoded}\x1b\\"))
}

#[derive(Default)]
struct TerminalProtocolParser {
    pending: Vec<u8>,
    active_request: Option<String>,
    sequence: u64,
}

impl TerminalProtocolParser {
    fn push(&mut self, bytes: &[u8]) -> Vec<ProviderExecutionEvent> {
        self.pending.extend_from_slice(bytes);
        self.drain(false)
    }

    fn finish(&mut self) -> Vec<ProviderExecutionEvent> {
        self.drain(true)
    }

    fn drain(&mut self, eof: bool) -> Vec<ProviderExecutionEvent> {
        let mut events = Vec::new();
        loop {
            let Some(start) = find_bytes(&self.pending, OSC_PREFIX) else {
                let keep = if eof {
                    0
                } else {
                    longest_prefix_suffix(&self.pending, OSC_PREFIX)
                };
                let emit = self.pending.len().saturating_sub(keep);
                if emit > 0 {
                    events.push(terminal_chunk(&self.pending[..emit]));
                    self.pending.drain(..emit);
                }
                break;
            };
            if start > 0 {
                events.push(terminal_chunk(&self.pending[..start]));
                self.pending.drain(..start);
                continue;
            }
            let payload_start = OSC_PREFIX.len();
            let terminator = find_terminator(&self.pending[payload_start..])
                .map(|(offset, len)| (payload_start + offset, len));
            let Some((payload_end, terminator_len)) = terminator else {
                if eof || self.pending.len() > OSC_PREFIX.len() + MAX_PROTOCOL_PAYLOAD * 2 {
                    events.push(protocol_warning(
                        "unterminated or oversized terminal marker",
                    ));
                    self.pending.clear();
                }
                break;
            };
            let encoded = self.pending[payload_start..payload_end].to_vec();
            self.pending.drain(..payload_end + terminator_len);
            match self.decode(&encoded) {
                Ok(Some(interaction)) => {
                    events.push(ProviderExecutionEvent::TerminalInteraction { interaction })
                }
                Ok(None) => {}
                Err(message) => events.push(protocol_warning(&message)),
            }
        }
        events
    }

    fn decode(&mut self, encoded: &[u8]) -> Result<Option<TerminalInteraction>, String> {
        let decoded = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| "invalid terminal marker encoding".to_string())?;
        if decoded.len() > MAX_PROTOCOL_PAYLOAD {
            return Err("terminal marker payload exceeds 16 KiB".into());
        }
        let payload: TerminalProtocolPayload = serde_json::from_slice(&decoded)
            .map_err(|_| "invalid terminal marker payload".to_string())?;
        if payload.version != 1 {
            return Err(format!(
                "unsupported terminal marker version {}",
                payload.version
            ));
        }
        if payload.request_id.is_empty() || payload.request_id.len() > MAX_REQUEST_ID {
            return Err("terminal request id must contain 1-128 bytes".into());
        }
        self.sequence += 1;
        match payload.event.as_str() {
            "input_required" => {
                let prompt = payload.prompt.unwrap_or_default();
                if prompt.len() > MAX_PROMPT {
                    return Err("terminal prompt exceeds 8 KiB".into());
                }
                self.active_request = Some(payload.request_id.clone());
                Ok(Some(TerminalInteraction {
                    sequence: self.sequence,
                    request_id: payload.request_id,
                    state: TerminalInteractionState::InputRequired,
                    prompt: Some(prompt),
                }))
            }
            "input_accepted" => {
                if self.active_request.as_deref() != Some(payload.request_id.as_str()) {
                    return Err(format!(
                        "ignored input acceptance for inactive request {}",
                        payload.request_id
                    ));
                }
                self.active_request = None;
                Ok(Some(TerminalInteraction {
                    sequence: self.sequence,
                    request_id: payload.request_id,
                    state: TerminalInteractionState::InputAccepted,
                    prompt: None,
                }))
            }
            _ => Err(format!("unknown terminal marker event {}", payload.event)),
        }
    }
}

fn terminal_chunk(bytes: &[u8]) -> ProviderExecutionEvent {
    ProviderExecutionEvent::Chunk {
        stream: "terminal".into(),
        content: String::from_utf8_lossy(bytes).into_owned(),
    }
}

fn protocol_warning(message: &str) -> ProviderExecutionEvent {
    terminal_chunk(format!("\r\n[runinator: {message}]\r\n").as_bytes())
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn longest_prefix_suffix(bytes: &[u8], prefix: &[u8]) -> usize {
    (1..=bytes.len().min(prefix.len().saturating_sub(1)))
        .rev()
        .find(|length| bytes.ends_with(&prefix[..*length]))
        .unwrap_or(0)
}

fn find_terminator(bytes: &[u8]) -> Option<(usize, usize)> {
    bytes.iter().enumerate().find_map(|(index, byte)| {
        if *byte == 0x07 {
            Some((index, 1))
        } else if *byte == 0x1b && bytes.get(index + 1) == Some(&b'\\') {
            Some((index, 2))
        } else {
            None
        }
    })
}

fn disconnected_receiver() -> Receiver<ProviderTerminalControl> {
    let (sender, receiver) = std::sync::mpsc::channel();
    drop(sender);
    receiver
}

fn unavailable(error: impl fmt::Display) -> TerminalError {
    TerminalError::Unavailable(error.to_string())
}

#[cfg(test)]
mod protocol_tests {
    use super::*;

    fn content(events: &[ProviderExecutionEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                ProviderExecutionEvent::Chunk { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn markers_survive_arbitrary_read_boundaries_and_are_not_rendered() {
        let required = input_required_marker("password", "Enter password").unwrap();
        let accepted = input_accepted_marker("password").unwrap();
        let wire = format!("before{required}prompt{accepted}after");
        let mut parser = TerminalProtocolParser::default();
        let mut events = Vec::new();
        for chunk in wire.as_bytes().chunks(3) {
            events.extend(parser.push(chunk));
        }
        events.extend(parser.finish());

        assert_eq!(content(&events), "beforepromptafter");
        let interactions = events
            .iter()
            .filter_map(|event| match event {
                ProviderExecutionEvent::TerminalInteraction { interaction } => Some(interaction),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(interactions.len(), 2);
        assert_eq!(interactions[0].sequence, 1);
        assert_eq!(
            interactions[0].state,
            TerminalInteractionState::InputRequired
        );
        assert_eq!(interactions[0].prompt.as_deref(), Some("Enter password"));
        assert_eq!(interactions[1].sequence, 2);
        assert_eq!(
            interactions[1].state,
            TerminalInteractionState::InputAccepted
        );
    }

    #[test]
    fn unrelated_osc_sequences_pass_through() {
        let bytes = b"a\x1b]0;window title\x07b";
        let mut parser = TerminalProtocolParser::default();
        let mut events = parser.push(bytes);
        events.extend(parser.finish());
        assert_eq!(content(&events).as_bytes(), bytes);
    }

    #[test]
    fn mismatched_acceptance_is_visible_and_does_not_emit_state() {
        let required = input_required_marker("one", "First").unwrap();
        let accepted = input_accepted_marker("two").unwrap();
        let mut parser = TerminalProtocolParser::default();
        let events = parser.push(format!("{required}{accepted}").as_bytes());
        assert!(content(&events).contains("inactive request two"));
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, ProviderExecutionEvent::TerminalInteraction { .. }))
                .count(),
            1
        );
    }
}
