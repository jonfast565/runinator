// the dictionary doubles as documentation; some entries are only reachable under
// optional broker features or via lookup, so allow unused items in this bin crate.
#![allow(dead_code)]

use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the worker engine (RUNI2xx).

// runtime and loop lifecycle.
pub const RUNTIME_BUILD: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI201",
    "worker.runtime",
    "Failed to build worker runtime",
);
pub const SIGNAL_CTRL_C: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI202",
    "worker.signal.ctrl_c",
    "Failed to listen for Ctrl+C",
);
pub const LOOP_JOIN: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI203",
    "worker.loop.join",
    "Worker loop task failed to join",
);
pub const API_CLIENT: ErrorDescriptor =
    ErrorDescriptor::new("RUNI204", "worker.api.client", "Failed to build API client");
pub const CONCURRENCY_CLOSED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI205",
    "worker.concurrency.closed",
    "Concurrency semaphore closed",
);
pub const PROVIDER_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI206",
    "worker.provider.not_found",
    "Cannot find plugin or provider",
);
pub const REPLICA_REGISTER: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI207",
    "worker.replica.register",
    "Failed to register worker replica",
);
pub const RELAY_URL: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI208",
    "worker.agent.relay_url",
    "Cannot derive broker relay URL from the service URL",
);
pub const SHUTDOWN_TIMEOUT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI209",
    "worker.agent.shutdown_timeout",
    "Agent did not stop within its grace period",
);

// broker wiring.
pub const BROKER_INVALID_ENDPOINT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI210",
    "worker.broker.invalid_endpoint",
    "Invalid broker endpoint",
);
pub const BROKER_CLIENT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI211",
    "worker.broker.client",
    "Failed to build broker client",
);
pub const BROKER_UNKNOWN_BACKEND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI212",
    "worker.broker.unknown_backend",
    "Unknown broker backend",
);
pub const BROKER_KAFKA: ErrorDescriptor =
    ErrorDescriptor::new("RUNI213", "worker.broker.kafka", "Kafka broker error");
pub const BROKER_RABBITMQ: ErrorDescriptor =
    ErrorDescriptor::new("RUNI215", "worker.broker.rabbitmq", "RabbitMQ broker error");
pub const BROKER_OPERATION: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI217",
    "worker.broker.operation",
    "Broker operation failed",
);
pub const BROKER_FEATURE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI218",
    "worker.broker.feature_disabled",
    "Broker backend compiled out",
);

/// a packaged function's code could not be downloaded or unpacked onto this worker.
pub const FUNCTION_STAGING_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI220",
    "worker.function.staging_failed",
    "Could not stage packaged function",
);
/// an artifact archive did not match its digest, or tried to write outside its own directory.
/// treated as hostile rather than corrupt: both are reasons not to run it.
pub const FUNCTION_UNTRUSTED_ARCHIVE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI221",
    "worker.function.untrusted_archive",
    "Package archive failed verification",
);
/// the action carried a function binding, but this worker could not resolve its published version.
pub const FUNCTION_BINDING_UNRESOLVED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI222",
    "worker.function.binding_unresolved",
    "Function binding could not be resolved",
);

/// the agent spent its consecutive-reconnect budget against an unreachable service or broker and
/// stopped itself rather than retrying forever.
pub const RECONNECT_EXHAUSTED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI223",
    "worker.agent.reconnect_exhausted",
    "Agent disconnected after exhausting its reconnect attempts",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    RUNTIME_BUILD,
    SIGNAL_CTRL_C,
    LOOP_JOIN,
    API_CLIENT,
    CONCURRENCY_CLOSED,
    PROVIDER_NOT_FOUND,
    REPLICA_REGISTER,
    RELAY_URL,
    SHUTDOWN_TIMEOUT,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_RABBITMQ,
    BROKER_OPERATION,
    BROKER_FEATURE_DISABLED,
    FUNCTION_STAGING_FAILED,
    FUNCTION_UNTRUSTED_ARCHIVE,
    FUNCTION_BINDING_UNRESOLVED,
    RECONNECT_EXHAUSTED,
];

/// worker engine error dictionary.
pub struct WorkerErrors;

impl EngineErrors for WorkerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
