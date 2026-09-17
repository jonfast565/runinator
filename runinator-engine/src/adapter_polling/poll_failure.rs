#[allow(unused_imports)]
use super::*;

pub(super) struct PollFailure {
    pub(super) message: String,
    pub(super) retry_after_seconds: i64,
}

impl From<String> for PollFailure {
    fn from(message: String) -> Self {
        Self {
            message,
            retry_after_seconds: DEFAULT_RETRY_SECONDS,
        }
    }
}

impl From<runinator_adapter_client::AdapterClientError> for PollFailure {
    fn from(error: runinator_adapter_client::AdapterClientError) -> Self {
        match error {
            runinator_adapter_client::AdapterClientError::CircuitOpen {
                retry_after_seconds,
            } => Self {
                message: "adapter-host circuit is open".into(),
                retry_after_seconds: i64::try_from(retry_after_seconds)
                    .unwrap_or(MAX_INTERVAL_SECONDS)
                    .clamp(MIN_INTERVAL_SECONDS, MAX_INTERVAL_SECONDS),
            },
            error => Self::from(error.to_string()),
        }
    }
}
