//! Bounded, best-effort publication of structured process logs to the Runinator service.

use std::{
    cell::Cell,
    env,
    io::{self, Write},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicU64, AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    time::Duration,
};

use runinator_models::{
    api_routes::API_DIAGNOSTIC_LOGS,
    diagnostics::{RuntimeLogBatch, RuntimeLogRecord},
};
use tracing_subscriber::fmt::MakeWriter;

const MEMORY_LIMIT: usize = 8 * 1024 * 1024;
const BATCH_LIMIT: usize = 256;
const BATCH_BYTES: usize = 64 * 1024;

thread_local! {
    static EXPORTING: Cell<bool> = const { Cell::new(false) };
}

struct Publisher {
    sender: SyncSender<RuntimeLogRecord>,
    queued_bytes: Arc<AtomicUsize>,
    dropped: Arc<AtomicU64>,
    source: String,
}

#[derive(Clone)]
struct RemoteConfig {
    base: String,
    token: Option<String>,
}

static PUBLISHER: OnceLock<Publisher> = OnceLock::new();
static CONFIG: OnceLock<RemoteConfig> = OnceLock::new();

/// Supply a parsed service URL before the process tracing subscriber is installed.
pub fn configure(base: impl Into<String>, token: Option<String>) {
    let base = base.into();
    if base.trim().is_empty() {
        return;
    }
    let _ = CONFIG.set(RemoteConfig { base, token });
}

/// Prepare a remote writer from the process's existing service URL and API-key environment.
pub fn prepare(source: &str) -> Option<RemoteLogMakeWriter> {
    if let Some(publisher) = PUBLISHER.get() {
        return Some(RemoteLogMakeWriter {
            source: publisher.source.clone(),
        });
    }
    let configured = CONFIG.get().cloned();
    let base = configured
        .as_ref()
        .map(|config| config.base.clone())
        .or_else(|| env::var("RUNINATOR_SERVICE_URL").ok())
        .or_else(|| env::var("RUNINATOR_API_BASE_URL").ok())?
        .trim_end_matches('/')
        .to_string();
    if base.is_empty() {
        return None;
    }
    let endpoint = format!("{base}{API_DIAGNOSTIC_LOGS}");
    let token = configured
        .and_then(|config| config.token)
        .or_else(|| env::var("RUNINATOR_API_KEY").ok())
        .filter(|value| !value.is_empty());
    let (sender, receiver) = mpsc::sync_channel(4_096);
    let queued_bytes = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicU64::new(0));
    let publisher = Publisher {
        sender,
        queued_bytes: queued_bytes.clone(),
        dropped: dropped.clone(),
        source: source.to_string(),
    };
    if PUBLISHER.set(publisher).is_err() {
        return PUBLISHER.get().map(|publisher| RemoteLogMakeWriter {
            source: publisher.source.clone(),
        });
    }
    let _ = std::thread::Builder::new()
        .name("runinator-diagnostics-export".into())
        .spawn(move || {
            let client = reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .ok();
            let Some(client) = client else { return };
            let mut pending = None;
            loop {
                let first = match pending.take() {
                    Some(record) => record,
                    None => match receiver.recv() {
                        Ok(record) => record,
                        Err(_) => break,
                    },
                };
                let mut bytes = first.message.len();
                queued_bytes.fetch_sub(bytes, Ordering::Relaxed);
                let mut records = vec![first];
                while records.len() < BATCH_LIMIT && bytes < BATCH_BYTES {
                    let Ok(record) = receiver.try_recv() else {
                        break;
                    };
                    let record_bytes = record.message.len();
                    if bytes.saturating_add(record_bytes) > BATCH_BYTES {
                        pending = Some(record);
                        break;
                    }
                    bytes += record_bytes;
                    queued_bytes.fetch_sub(record_bytes, Ordering::Relaxed);
                    records.push(record);
                }
                records[0].dropped_before = dropped.swap(0, Ordering::Relaxed);
                let record_count = records.len() as u64;
                EXPORTING.with(|flag| flag.set(true));
                let mut request = client.post(&endpoint).json(&RuntimeLogBatch { records });
                if let Some(token) = &token {
                    request = request.bearer_auth(token);
                }
                if request
                    .send()
                    .and_then(|response| response.error_for_status())
                    .is_err()
                {
                    dropped.fetch_add(record_count, Ordering::Relaxed);
                }
                EXPORTING.with(|flag| flag.set(false));
            }
        });
    Some(RemoteLogMakeWriter {
        source: source.to_string(),
    })
}

#[derive(Clone)]
pub struct RemoteLogMakeWriter {
    source: String,
}

impl<'a> MakeWriter<'a> for RemoteLogMakeWriter {
    type Writer = RemoteLogWriter;

    fn make_writer(&'a self) -> Self::Writer {
        RemoteLogWriter {
            source: self.source.clone(),
            buffer: Vec::new(),
        }
    }
}

pub struct RemoteLogWriter {
    source: String,
    buffer: Vec<u8>,
}

impl Write for RemoteLogWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Drop for RemoteLogWriter {
    fn drop(&mut self) {
        if EXPORTING.with(Cell::get) {
            return;
        }
        let Some(publisher) = PUBLISHER.get() else {
            return;
        };
        for line in String::from_utf8_lossy(&self.buffer)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
        {
            let message = line.chars().take(16 * 1024).collect::<String>();
            let size = message.len();
            let previous = publisher.queued_bytes.fetch_add(size, Ordering::Relaxed);
            if previous.saturating_add(size) > MEMORY_LIMIT {
                publisher.queued_bytes.fetch_sub(size, Ordering::Relaxed);
                publisher.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            let (level, target) = level_and_target(&message, &self.source);
            let record = RuntimeLogRecord {
                event_id: uuid::Uuid::now_v7(),
                occurred_at: chrono::Utc::now(),
                source: self.source.clone(),
                runtime_id: env::var("RUNINATOR_RUNTIME_ID").ok(),
                replica_id: env::var("RUNINATOR_REPLICA_ID")
                    .ok()
                    .and_then(|value| value.parse().ok()),
                level: level.into(),
                target,
                message,
                dropped_before: 0,
                workflow_run_id: None,
                effect_id: None,
                trace_id: None,
            };
            if publisher.sender.try_send(record).is_err() {
                publisher.queued_bytes.fetch_sub(size, Ordering::Relaxed);
                publisher.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

fn level_and_target(line: &str, fallback_target: &str) -> (&'static str, String) {
    for (needle, level) in [
        (" ERROR ", "error"),
        (" WARN ", "warn"),
        (" INFO ", "info"),
        (" DEBUG ", "debug"),
        (" TRACE ", "trace"),
    ] {
        if let Some(index) = line.find(needle) {
            let remainder = &line[index + needle.len()..];
            let target = remainder
                .split_once(": ")
                .map(|(target, _)| target.trim())
                .filter(|target| !target.is_empty())
                .unwrap_or(fallback_target)
                .to_string();
            return (level, target);
        }
    }
    ("info", fallback_target.to_string())
}

/// Number of records or batches discarded by the bounded exporter in this process.
pub fn dropped() -> u64 {
    PUBLISHER
        .get()
        .map_or(0, |publisher| publisher.dropped.load(Ordering::Relaxed))
}

#[cfg(test)]
#[path = "remote_logs_tests.rs"]
mod tests;
