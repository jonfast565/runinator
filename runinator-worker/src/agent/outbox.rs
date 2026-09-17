//! durable buffering for terminal worker results when the broker link is unavailable.

use std::{
    collections::VecDeque,
    fmt, fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use runinator_broker::{Broker, EffectResultMessage};
use runinator_models::errors::SendableError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_MAX_ENTRIES: usize = 10_000;
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const DEFAULT_MAX_ATTEMPTS: u32 = 20;

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
    let published = broker.publish_effect_result(entry.message.0).await;
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

mod outbox_entry;
pub use outbox_entry::OutboxEntry;

mod outbox_message;
pub use outbox_message::OutboxMessage;

mod result_outbox;
pub use result_outbox::ResultOutbox;

mod noop_outbox;
pub use noop_outbox::NoopOutbox;

mod file_state;
use file_state::FileState;

mod file_outbox;
pub use file_outbox::FileOutbox;
