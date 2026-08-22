use runinator_models::errors::{EngineErrors, ErrorDescriptor};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BrokerError {
    #[error("BROKER001 - duplicate message for key {0}")]
    Duplicate(String),
    #[error("BROKER002 - delivery not found: {0}")]
    UnknownDelivery(uuid::Uuid),
    #[error("BROKER003 - operation not implemented: {0}")]
    NotImplemented(&'static str),
    #[error("BROKER004 - workflow effect channels unsupported: {0}")]
    WorkflowEffectsUnsupported(String),
    #[error("BROKER005 - internal broker error: {0}")]
    Internal(String),
    #[error("BROKER006 - {0} broker feature disabled: rebuild with --features {0}")]
    FeatureDisabled(&'static str),
    #[error("BROKER007 - consumer stream ended; connection may have dropped")]
    ConsumerStreamEnded,
    /// the broker refused our credential. distinct from [`BrokerError::Internal`] because it is not
    /// transient: retrying with the same credential will fail identically forever, so a transport
    /// that reconnects on its own must stop treating it as a blip and say so.
    #[error("BROKER008 - broker rejected our credential: {0}")]
    Unauthorized(String),
}

// numbered error dictionary for the broker engine.
pub const DUPLICATE: ErrorDescriptor =
    ErrorDescriptor::new("BROKER001", "broker.duplicate", "Duplicate message for key");
pub const UNKNOWN_DELIVERY: ErrorDescriptor =
    ErrorDescriptor::new("BROKER002", "broker.unknown_delivery", "Delivery not found");
pub const NOT_IMPLEMENTED: ErrorDescriptor = ErrorDescriptor::new(
    "BROKER003",
    "broker.not_implemented",
    "Operation not implemented",
);
pub const WORKFLOW_EFFECTS_UNSUPPORTED: ErrorDescriptor = ErrorDescriptor::new(
    "BROKER004",
    "broker.workflow_effects_unsupported",
    "Workflow effect channels unsupported",
);
pub const INTERNAL: ErrorDescriptor =
    ErrorDescriptor::new("BROKER005", "broker.internal", "Internal broker error");
pub const FEATURE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "BROKER006",
    "broker.feature_disabled",
    "Broker backend feature disabled",
);
pub const CONSUMER_STREAM_ENDED: ErrorDescriptor = ErrorDescriptor::new(
    "BROKER007",
    "broker.consumer_stream_ended",
    "Consumer stream ended; connection may have dropped",
);

pub const UNAUTHORIZED: ErrorDescriptor = ErrorDescriptor::new(
    "BROKER008",
    "broker.unauthorized",
    "Broker rejected our credential",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    DUPLICATE,
    UNKNOWN_DELIVERY,
    NOT_IMPLEMENTED,
    WORKFLOW_EFFECTS_UNSUPPORTED,
    INTERNAL,
    FEATURE_DISABLED,
    CONSUMER_STREAM_ENDED,
    UNAUTHORIZED,
];

impl EngineErrors for BrokerError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
