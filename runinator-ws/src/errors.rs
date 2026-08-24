use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the web service HTTP surface (RUNI17x). the pack-import, debug,
// control, and replay descriptors (RUNI12x-14x) moved to runinator-engine with the repository code
// That crate emits them; this dictionary keeps only the HTTP and broker-wiring codes WS owns.

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

// the `/ws/broker` relay's refusals. these previously answered with bare prose, which meant a
// misconfigured or misbehaving runtime hot-looping against the relay was indistinguishable from a
// healthy one in anything but a log grep. relay clients surface these to their operator verbatim.
pub const RELAY_NOT_EXCLUSIVE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI177",
    "ws.relay.not_exclusive",
    "Relay requires an exclusive consumer profile",
);
pub const RELAY_OPERATION_REFUSED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI178",
    "ws.relay.operation_refused",
    "Operation not permitted over the relay",
);
pub const RELAY_REPLICA_NOT_OWNED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI179",
    "ws.relay.replica_not_owned",
    "Replica is not owned by the connecting identity",
);
pub const RELAY_UNKNOWN_REPLICA: ErrorDescriptor =
    ErrorDescriptor::new("RUNI180", "ws.relay.unknown_replica", "Unknown replica");
pub const RELAY_REPLICA_LOOKUP: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI181",
    "ws.relay.replica_lookup",
    "Failed to resolve the relay's replica",
);
pub const RELAY_BUSY: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI182",
    "ws.relay.busy",
    "Too many relay requests in flight on this connection",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    BROKER_WORKFLOW_RESULTS,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_RABBITMQ,
    RELAY_NOT_EXCLUSIVE,
    RELAY_OPERATION_REFUSED,
    RELAY_REPLICA_NOT_OWNED,
    RELAY_UNKNOWN_REPLICA,
    RELAY_REPLICA_LOOKUP,
    RELAY_BUSY,
];

/// web service engine error dictionary.
pub struct WsErrors;

impl EngineErrors for WsErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
