use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the web service HTTP surface (RUNI17x). the pack-import, debug,
// control, and replay descriptors (RUNI12x-14x) moved to runinator-engine with the repository code
// that emits them; this dictionary keeps only the HTTP/broker-wiring codes ws owns.

// broker wiring.
pub const BROKER_WORKFLOW_RESULTS: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI170",
    "ws.broker.workflow_results",
    "Workflow result channel unavailable",
);
pub const BROKER_INVALID_ENDPOINT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI171",
    "ws.broker.invalid_endpoint",
    "Invalid broker endpoint",
);
pub const BROKER_CLIENT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI172",
    "ws.broker.client",
    "Failed to build broker client",
);
pub const BROKER_UNKNOWN_BACKEND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI173",
    "ws.broker.unknown_backend",
    "Unknown broker backend",
);
pub const BROKER_KAFKA: ErrorDescriptor =
    ErrorDescriptor::new("RUNI174", "ws.broker.kafka", "Kafka broker error");
pub const BROKER_RABBITMQ: ErrorDescriptor =
    ErrorDescriptor::new("RUNI176", "ws.broker.rabbitmq", "RabbitMQ broker error");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    BROKER_WORKFLOW_RESULTS,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_RABBITMQ,
];

/// web service engine error dictionary.
pub struct WsErrors;

impl EngineErrors for WsErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
