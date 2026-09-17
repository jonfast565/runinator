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
            body_bytes: env::parse_positive_or(
                "RUNINATOR_ADAPTER_BODY_LIMIT_BYTES",
                DEFAULT_BODY_LIMIT,
            ),
            output_bytes: env::parse_positive_or(
                "RUNINATOR_ADAPTER_OUTPUT_LIMIT_BYTES",
                DEFAULT_OUTPUT_LIMIT,
            ),
            event_count: env::parse_positive_or(
                "RUNINATOR_ADAPTER_EVENT_LIMIT",
                DEFAULT_EVENT_LIMIT,
            ),
            timeout: Duration::from_millis(env::parse_positive_or(
                "RUNINATOR_ADAPTER_PLUGIN_TIMEOUT_MS",
                DEFAULT_TIMEOUT.as_millis() as u64,
            )),
        }
    }
}
