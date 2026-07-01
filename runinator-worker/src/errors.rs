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

pub const DICTIONARY: &[ErrorDescriptor] = &[
    RUNTIME_BUILD,
    SIGNAL_CTRL_C,
    LOOP_JOIN,
    API_CLIENT,
    CONCURRENCY_CLOSED,
    PROVIDER_NOT_FOUND,
    REPLICA_REGISTER,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_RABBITMQ,
    BROKER_OPERATION,
];

/// worker engine error dictionary.
pub struct WorkerErrors;

impl EngineErrors for WorkerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
