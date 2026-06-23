use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// default interval between liveness-file touches; matches the k8s exec probe cadence.
pub const DEFAULT_LIVENESS_INTERVAL: Duration = Duration::from_secs(30);

/// writes an empty file at path to signal the process is alive.
pub fn touch_liveness(path: impl AsRef<Path>) -> std::io::Result<()> {
    std::fs::write(path, b"")
}

/// spawns a task that touches the liveness file at path on interval until shutdown is notified.
/// returns none when path is blank so callers can disable the probe with an empty string.
pub fn spawn_liveness(
    path: &str,
    interval: Duration,
    shutdown: Arc<Notify>,
) -> Option<JoinHandle<()>> {
    if path.trim().is_empty() {
        return None;
    }
    let path = path.to_string();
    Some(tokio::spawn(async move {
        loop {
            let _ = touch_liveness(&path);
            tokio::select! {
                _ = shutdown.notified() => return,
                _ = tokio::time::sleep(interval) => {}
            }
        }
    }))
}

#[cfg(test)]
mod tests;
