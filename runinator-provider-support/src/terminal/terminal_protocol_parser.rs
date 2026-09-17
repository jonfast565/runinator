#[allow(unused_imports)]
use super::*;

#[derive(Default)]
pub(super) struct TerminalProtocolParser {
    pub(super) pending: Vec<u8>,
    pub(super) active_request: Option<String>,
    pub(super) sequence: u64,
}

impl TerminalProtocolParser {
    pub(super) fn push(&mut self, bytes: &[u8]) -> Vec<ProviderExecutionEvent> {
        self.pending.extend_from_slice(bytes);
        self.drain(false)
    }

    pub(super) fn finish(&mut self) -> Vec<ProviderExecutionEvent> {
        self.drain(true)
    }

    pub(super) fn drain(&mut self, eof: bool) -> Vec<ProviderExecutionEvent> {
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

    pub(super) fn decode(&mut self, encoded: &[u8]) -> Result<Option<TerminalInteraction>, String> {
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
