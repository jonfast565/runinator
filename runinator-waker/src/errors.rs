use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the waker engine (RUNI3xx).

pub const SIGNAL_CTRL_C: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI301",
    "waker.signal.ctrl_c",
    "Failed to listen for Ctrl+C",
);
pub const REPLICA_REGISTER: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI302",
    "waker.replica.register",
    "Failed to register waker replica",
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
pub const BROKER_RABBITMQ: ErrorDescriptor =
    ErrorDescriptor::new("RUNI315", "waker.broker.rabbitmq", "RabbitMQ broker error");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    SIGNAL_CTRL_C,
    REPLICA_REGISTER,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_RABBITMQ,
];

/// waker engine error dictionary.
pub struct WakerErrors;

impl EngineErrors for WakerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
