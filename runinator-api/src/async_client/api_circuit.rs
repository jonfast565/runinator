#[allow(unused_imports)]
use super::*;

/// The HTTP client's circuit state is shared by every clone of one API client. It intentionally
/// remains process-local: service discovery or a process restart must not turn a transient remote
/// problem into cluster-wide persisted unavailability.
#[derive(Clone)]
pub(super) struct ApiCircuit {
    pub(super) enabled: bool,
    pub(super) cooldown: Duration,
    pub(super) layer: HttpCircuitLayer,
}

impl ApiCircuit {
    pub(super) fn from_env() -> Self {
        let enabled = env::flag_or("RUNINATOR_API_CIRCUIT_BREAKER_ENABLED", true);
        let failures = env::parse_positive_or(
            "RUNINATOR_API_CIRCUIT_BREAKER_FAILURE_THRESHOLD",
            DEFAULT_CIRCUIT_FAILURE_THRESHOLD,
        );
        let cooldown = Duration::from_secs(env::parse_positive_or(
            "RUNINATOR_API_CIRCUIT_BREAKER_COOLDOWN_SECONDS",
            DEFAULT_CIRCUIT_COOLDOWN_SECONDS,
        ));
        Self::new(enabled, failures, cooldown)
    }

    pub(super) fn new(enabled: bool, failures: usize, cooldown: Duration) -> Self {
        let (layer, _) = CircuitBreakerLayer::builder()
            .name("runinator_api")
            .consecutive_failures(failures.max(1))
            .wait_duration_in_open(cooldown)
            .permitted_calls_in_half_open(1)
            .failure_classifier(outbound_failure as HttpClassifier)
            .on_state_transition(|from, to| {
                log::warn!("Runinator API circuit transitioned from {from:?} to {to:?}");
                metrics::counter!(
                    "runinator_api_circuit_breaker_transitions_total",
                    "target" => "runinator_api",
                    "from" => format!("{from:?}"),
                    "to" => format!("{to:?}"),
                )
                .increment(1);
            })
            .on_call_rejected(|| {
                log::warn!("Runinator API request rejected because the local circuit is open");
                metrics::counter!(
                    "runinator_api_circuit_breaker_rejections_total",
                    "target" => "runinator_api",
                )
                .increment(1);
            })
            .build_with_handle()
            .expect("API circuit breaker uses a validated static configuration");
        Self {
            enabled,
            cooldown,
            layer,
        }
    }
}
