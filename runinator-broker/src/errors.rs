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
    #[error("BROKER004 - workflow result channels unsupported: {0}")]
    WorkflowResultsUnsupported(String),
    #[error("BROKER005 - internal broker error: {0}")]
    Internal(String),
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
pub const WORKFLOW_RESULTS_UNSUPPORTED: ErrorDescriptor = ErrorDescriptor::new(
    "BROKER004",
    "broker.workflow_results_unsupported",
    "Workflow result channels unsupported",
);
pub const INTERNAL: ErrorDescriptor =
    ErrorDescriptor::new("BROKER005", "broker.internal", "Internal broker error");

pub const DICTIONARY: &[ErrorDescriptor] = &[
    DUPLICATE,
    UNKNOWN_DELIVERY,
    NOT_IMPLEMENTED,
    WORKFLOW_RESULTS_UNSUPPORTED,
    INTERNAL,
];

impl EngineErrors for BrokerError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
