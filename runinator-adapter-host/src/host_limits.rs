#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) struct HostLimits {
    pub(super) body_bytes: usize,
    pub(super) output_bytes: usize,
    pub(super) event_count: usize,
    pub(super) timeout: Duration,
}

impl HostLimits {
    pub(super) fn from_env() -> Self {
        Self {
            body_bytes: positive_env_usize("RUNINATOR_ADAPTER_BODY_LIMIT_BYTES")
                .unwrap_or(DEFAULT_BODY_LIMIT),
            output_bytes: positive_env_usize("RUNINATOR_ADAPTER_OUTPUT_LIMIT_BYTES")
                .unwrap_or(DEFAULT_OUTPUT_LIMIT),
            event_count: positive_env_usize("RUNINATOR_ADAPTER_EVENT_LIMIT")
                .unwrap_or(DEFAULT_EVENT_LIMIT),
            timeout: Duration::from_millis(
                positive_env_u64("RUNINATOR_ADAPTER_PLUGIN_TIMEOUT_MS")
                    .unwrap_or(DEFAULT_TIMEOUT.as_millis() as u64),
            ),
        }
    }
}
