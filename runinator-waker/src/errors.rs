use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the waker engine (RUNI3xx).

pub const SIGNAL_CTRL_C: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI301",
    "waker.signal.ctrl_c",
    "Failed to listen for Ctrl+C",
);

// broker wiring.
pub const BROKER_INVALID_ENDPOINT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI310",
    "waker.broker.invalid_endpoint",
    "Invalid broker endpoint",
);
pub const BROKER_CLIENT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI311",
    "waker.broker.client",
    "Failed to build broker client",
);
pub const BROKER_UNKNOWN_BACKEND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI312",
    "waker.broker.unknown_backend",
    "Unknown broker backend",
);
pub const BROKER_KAFKA: ErrorDescriptor =
    ErrorDescriptor::new("RUNI313", "waker.broker.kafka", "Kafka broker error");
pub const BROKER_KAFKA_FEATURE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI314",
    "waker.broker.kafka_feature_disabled",
    "Kafka broker support not compiled in",
);
pub const BROKER_RABBITMQ: ErrorDescriptor =
    ErrorDescriptor::new("RUNI315", "waker.broker.rabbitmq", "RabbitMQ broker error");
pub const BROKER_RABBITMQ_FEATURE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI316",
    "waker.broker.rabbitmq_feature_disabled",
    "RabbitMQ broker support not compiled in",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    SIGNAL_CTRL_C,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_KAFKA_FEATURE_DISABLED,
    BROKER_RABBITMQ,
    BROKER_RABBITMQ_FEATURE_DISABLED,
];

/// waker engine error dictionary.
pub struct WakerErrors;

impl EngineErrors for WakerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
