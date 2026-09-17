//! Restore and snapshot isolated portable workspaces around provider execution.
use runinator_api::{
    ApiError,
    capabilities::{WorkspaceCheckoutClient, WorkspaceObjectTransport},
};
use runinator_models::{
    errors::{SendableError, WORKSPACE_INVALID},
    value::Value,
    workspaces::*,
};

const WORKSPACE_CLAIM_RETRY_INITIAL_DELAY: std::time::Duration =
    std::time::Duration::from_millis(25);
const WORKSPACE_CLAIM_RETRY_MAX_DELAY: std::time::Duration = std::time::Duration::from_millis(250);
const WORKSPACE_CLAIM_SYNC_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

async fn download_workspace_checkout_after_claim(
    api: &dyn WorkspaceCheckoutClient,
    checkout_id: uuid::Uuid,
    replica_id: uuid::Uuid,
    deadline: std::time::Instant,
) -> Result<Vec<u8>, SendableError> {
    retry_pending_workspace_claim(deadline, |remaining| {
        api.download_workspace_checkout(checkout_id, replica_id, remaining)
    })
    .await
}

async fn retry_pending_workspace_claim<F, Fut>(
    deadline: std::time::Instant,
    mut download: F,
) -> Result<Vec<u8>, SendableError>
where
    F: FnMut(std::time::Duration) -> Fut,
    Fut: std::future::Future<Output = runinator_api::Result<Vec<u8>>>,
{
    let claim_deadline = std::cmp::min(
        deadline,
        std::time::Instant::now() + WORKSPACE_CLAIM_SYNC_TIMEOUT,
    );
    let mut delay = WORKSPACE_CLAIM_RETRY_INITIAL_DELAY;
    loop {
        let remaining = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(workspace_attempt_timed_out)?;
        match download(remaining).await {
            Ok(bytes) => return Ok(bytes),
            Err(error)
                if workspace_claim_is_pending(&error)
                    && std::time::Instant::now() < claim_deadline =>
            {
                let wait =
                    delay.min(claim_deadline.saturating_duration_since(std::time::Instant::now()));
                tokio::time::sleep(wait).await;
                delay = delay.saturating_mul(2).min(WORKSPACE_CLAIM_RETRY_MAX_DELAY);
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn workspace_claim_is_pending(error: &ApiError) -> bool {
    matches!(
        error,
        ApiError::Http {
            status: reqwest::StatusCode::CONFLICT,
            message,
            ..
        } if message.contains("replica has not claimed this active attempt")
    )
}

fn workspace_attempt_timed_out() -> SendableError {
    runinator_workspace::storage::Error::Io(std::io::Error::new(
        std::io::ErrorKind::TimedOut,
        "workspace attempt deadline exceeded",
    ))
    .into()
}

fn cache_root() -> Result<std::path::PathBuf, SendableError> {
    runinator_platform::app_data::app_data_path("portable-workspaces")
}

pub async fn cleanup_expired() {
    let result = tokio::task::spawn_blocking(|| -> Result<(), SendableError> {
        let root = cache_root()?;
        if !root.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(root)? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(expires) = name
                .to_str()
                .and_then(|name| name.strip_prefix("lease-"))
                .and_then(|name| name.split('-').next())
                .and_then(|value| value.parse::<i64>().ok())
            else {
                continue;
            };
            if expires < chrono::Utc::now().timestamp() && entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            }
        }
        Ok(())
    })
    .await;
    if let Err(error) = result.unwrap_or_else(|error| Err(error.into())) {
        tracing::warn!(%error, "failed to remove expired workspace working copies");
    }
}

#[cfg(test)]
#[path = "durable_workspace_tests.rs"]
mod durable_workspace_tests;

mod workspace_phase_reporter;
pub use workspace_phase_reporter::WorkspacePhaseReporter;

mod workspace_phase_timer;
pub use workspace_phase_timer::WorkspacePhaseTimer;

mod active_workspace;
pub use active_workspace::ActiveWorkspace;

mod workspace_restore_options;
use workspace_restore_options::WorkspaceRestoreOptions;
