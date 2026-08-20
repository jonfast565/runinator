//! durable buffering for terminal worker results when the broker link is unavailable.

use std::{
    collections::VecDeque,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use runinator_broker::{Broker, EffectResultMessage, ResultMessage};
use runinator_models::errors::SendableError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_MAX_ENTRIES: usize = 10_000;
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_ATTEMPTS: u32 = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboxEntry {
    pub id: Uuid,
    pub message: OutboxMessage,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OutboxMessage {
    Legacy(ResultMessage),
    Effect(EffectResultMessage),
}

#[derive(Debug)]
pub enum OutboxError {
    Disabled,
    Full,
    Io(io::Error),
    InvalidData(String),
}

impl fmt::Display for OutboxError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled => formatter.write_str("result outbox is disabled"),
            Self::Full => formatter.write_str("result outbox reached its configured hard cap"),
            Self::Io(err) => write!(formatter, "result outbox I/O failed: {err}"),
            Self::InvalidData(err) => write!(formatter, "result outbox is invalid: {err}"),
        }
    }
}

impl std::error::Error for OutboxError {}

impl From<io::Error> for OutboxError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub trait ResultOutbox: Send + Sync {
    /// fsync a terminal status or artifact before its action delivery may be acknowledged.
    fn append(&self, message: ResultMessage) -> Result<(), OutboxError>;
    fn append_effect(&self, message: EffectResultMessage) -> Result<(), OutboxError>;
    fn next(&self) -> Result<Option<OutboxEntry>, OutboxError>;
    fn acknowledge(&self, id: Uuid) -> Result<(), OutboxError>;
    fn record_failure(&self, id: Uuid, error: String) -> Result<(), OutboxError>;
    fn depth(&self) -> u64;
    fn is_full(&self) -> bool;
}

#[derive(Default)]
pub struct NoopOutbox;

impl ResultOutbox for NoopOutbox {
    fn append(&self, _message: ResultMessage) -> Result<(), OutboxError> {
        Err(OutboxError::Disabled)
    }

    fn append_effect(&self, _message: EffectResultMessage) -> Result<(), OutboxError> {
        Err(OutboxError::Disabled)
    }

    fn next(&self) -> Result<Option<OutboxEntry>, OutboxError> {
        Ok(None)
    }

    fn acknowledge(&self, _id: Uuid) -> Result<(), OutboxError> {
        Ok(())
    }

    fn record_failure(&self, _id: Uuid, _error: String) -> Result<(), OutboxError> {
        Ok(())
    }

    fn depth(&self) -> u64 {
        0
    }

    fn is_full(&self) -> bool {
        false
    }
}

struct FileState {
    entries: VecDeque<OutboxEntry>,
    bytes: u64,
}

pub struct FileOutbox {
    path: PathBuf,
    dead_letter_path: PathBuf,
    max_entries: usize,
    max_bytes: u64,
    max_attempts: u32,
    state: Mutex<FileState>,
}

impl FileOutbox {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, OutboxError> {
        Self::with_limits(
            path,
            DEFAULT_MAX_ENTRIES,
            DEFAULT_MAX_BYTES,
            DEFAULT_MAX_ATTEMPTS,
        )
    }

    pub fn with_limits(
        path: impl Into<PathBuf>,
        max_entries: usize,
        max_bytes: u64,
        max_attempts: u32,
    ) -> Result<Self, OutboxError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let dead_letter_path = path.with_extension("dead-letter.jsonl");
        let raw = match fs::read(&path) {
            Ok(raw) => {
                set_private_file_permissions(&path)?;
                raw
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        let mut entries = VecDeque::new();
        for (line_number, line) in raw.split(|byte| *byte == b'\n').enumerate() {
            if line.iter().all(u8::is_ascii_whitespace) {
                continue;
            }
            let entry = serde_json::from_slice(line).map_err(|err| {
                OutboxError::InvalidData(format!("line {}: {err}", line_number + 1))
            })?;
            entries.push_back(entry);
        }
        Ok(Self {
            path,
            dead_letter_path,
            max_entries: max_entries.max(1),
            max_bytes: max_bytes.max(1),
            max_attempts: max_attempts.max(1),
            state: Mutex::new(FileState {
                entries,
                bytes: raw.len() as u64,
            }),
        })
    }

    fn rewrite(&self, state: &mut FileState) -> Result<(), OutboxError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let temp = parent.join(format!(
            ".{}.{}.tmp",
            self.path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("result-outbox"),
            Uuid::new_v4()
        ));
        let result = (|| -> Result<u64, OutboxError> {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temp)?;
            set_private_file_permissions(&temp)?;
            let mut bytes = 0u64;
            for entry in &state.entries {
                let encoded = serde_json::to_vec(entry)
                    .map_err(|err| OutboxError::InvalidData(err.to_string()))?;
                file.write_all(&encoded)?;
                file.write_all(b"\n")?;
                bytes = bytes.saturating_add(encoded.len() as u64 + 1);
            }
            file.sync_all()?;
            fs::rename(&temp, &self.path)?;
            sync_directory(parent)?;
            Ok(bytes)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        state.bytes = result?;
        Ok(())
    }

    fn append_dead_letter(&self, entry: &OutboxEntry) -> Result<(), OutboxError> {
        let encoded =
            serde_json::to_vec(entry).map_err(|err| OutboxError::InvalidData(err.to_string()))?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.dead_letter_path)?;
        set_private_file_permissions(&self.dead_letter_path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        sync_directory(
            self.dead_letter_path
                .parent()
                .unwrap_or_else(|| Path::new(".")),
        )?;
        Ok(())
    }
}

impl ResultOutbox for FileOutbox {
    fn append(&self, message: ResultMessage) -> Result<(), OutboxError> {
        self.append_message(OutboxMessage::Legacy(message))
    }

    fn append_effect(&self, message: EffectResultMessage) -> Result<(), OutboxError> {
        self.append_message(OutboxMessage::Effect(message))
    }

    fn next(&self) -> Result<Option<OutboxEntry>, OutboxError> {
        Ok(self
            .state
            .lock()
            .expect("result outbox lock poisoned")
            .entries
            .front()
            .cloned())
    }

    fn acknowledge(&self, id: Uuid) -> Result<(), OutboxError> {
        let mut state = self.state.lock().expect("result outbox lock poisoned");
        if state.entries.front().is_some_and(|entry| entry.id == id) {
            state.entries.pop_front();
            self.rewrite(&mut state)?;
        }
        Ok(())
    }

    fn record_failure(&self, id: Uuid, error: String) -> Result<(), OutboxError> {
        let mut state = self.state.lock().expect("result outbox lock poisoned");
        let Some(entry) = state.entries.front_mut().filter(|entry| entry.id == id) else {
            return Ok(());
        };
        entry.attempts = entry.attempts.saturating_add(1);
        entry.last_error = Some(error);
        if entry.attempts >= self.max_attempts {
            let dead_letter = entry.clone();
            self.append_dead_letter(&dead_letter)?;
            state.entries.pop_front();
        }
        self.rewrite(&mut state)
    }

    fn depth(&self) -> u64 {
        self.state
            .lock()
            .expect("result outbox lock poisoned")
            .entries
            .len() as u64
    }

    fn is_full(&self) -> bool {
        let state = self.state.lock().expect("result outbox lock poisoned");
        state.entries.len() >= self.max_entries || state.bytes >= self.max_bytes
    }
}

impl FileOutbox {
    fn append_message(&self, message: OutboxMessage) -> Result<(), OutboxError> {
        let mut entry = OutboxEntry {
            id: Uuid::now_v7(),
            message,
            attempts: 0,
            last_error: None,
        };
        let encoded =
            serde_json::to_vec(&entry).map_err(|err| OutboxError::InvalidData(err.to_string()))?;
        let mut state = self.state.lock().expect("result outbox lock poisoned");
        if state.entries.len() >= self.max_entries
            || state.bytes.saturating_add(encoded.len() as u64 + 1) > self.max_bytes
        {
            // work already in flight must never be nacked and re-executed merely because older
            // results filled the pending queue. durably dead-letter this overflow, leave the queue
            // full (which puts the agent into draining), and acknowledge only after this fsync.
            entry.attempts = self.max_attempts;
            entry.last_error = Some("result outbox capacity exceeded".to_string());
            self.append_dead_letter(&entry)?;
            return Ok(());
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        set_private_file_permissions(&self.path)?;
        file.write_all(&encoded)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        sync_directory(self.path.parent().unwrap_or_else(|| Path::new(".")))?;
        state.bytes = state.bytes.saturating_add(encoded.len() as u64 + 1);
        state.entries.push_back(entry);
        Ok(())
    }
}

fn sync_directory(path: &Path) -> io::Result<()> {
    let directory = fs::File::open(path)?;
    directory.sync_all()
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// drain everything recorded before process start before the worker accepts another action. returns
/// `false` only when shutdown was requested while retrying.
pub async fn drain_before_work(
    outbox: &dyn ResultOutbox,
    broker: &dyn Broker,
    shutdown: &tokio::sync::Notify,
) -> Result<bool, SendableError> {
    let mut delay = std::time::Duration::from_secs(1);
    while outbox.depth() > 0 {
        match drain_one(outbox, broker).await? {
            true => delay = std::time::Duration::from_secs(1),
            false => {
                tokio::select! {
                    _ = shutdown.notified() => return Ok(false),
                    _ = tokio::time::sleep(delay) => {}
                }
                delay = (delay * 2).min(std::time::Duration::from_secs(60));
            }
        }
    }
    Ok(true)
}

/// continuously redrive records appended after startup. dedupe keys make a publish-then-crash
/// harmless: the server applies the same event id only once.
pub async fn drain_forever(
    outbox: &dyn ResultOutbox,
    broker: &dyn Broker,
    shutdown: &tokio::sync::Notify,
) -> Result<(), SendableError> {
    loop {
        if outbox.depth() == 0 {
            tokio::select! {
                _ = shutdown.notified() => return Ok(()),
                _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
            }
            continue;
        }
        if !drain_one(outbox, broker).await? {
            tokio::select! {
                _ = shutdown.notified() => return Ok(()),
                _ = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
            }
        }
    }
}

async fn drain_one(outbox: &dyn ResultOutbox, broker: &dyn Broker) -> Result<bool, SendableError> {
    let Some(entry) = outbox
        .next()
        .map_err(|err| Box::new(err) as SendableError)?
    else {
        return Ok(true);
    };
    let published = match entry.message {
        OutboxMessage::Legacy(message) => broker.publish_result(message).await,
        OutboxMessage::Effect(message) => broker.publish_effect_result(message).await,
    };
    match published {
        Ok(()) => {
            crate::metrics::result_publish("replayed");
            outbox
                .acknowledge(entry.id)
                .map_err(|err| Box::new(err) as SendableError)?;
            Ok(true)
        }
        Err(err) => {
            crate::metrics::result_publish("error");
            outbox
                .record_failure(entry.id, err.to_string())
                .map_err(|err| Box::new(err) as SendableError)?;
            Ok(false)
        }
    }
}

#[cfg(test)]
#[path = "outbox_tests.rs"]
mod tests;
