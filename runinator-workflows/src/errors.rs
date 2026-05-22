use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowValidationError {
    #[error("workflow definition.nodes must be an array")]
    MissingNodes,
    #[error("workflow definition.start must name the first node")]
    MissingStart,
    #[error("workflow node '{0}' is duplicated")]
    DuplicateNode(String),
    #[error("workflow start node '{0}' does not exist")]
    MissingStartNode(String),
    #[error("workflow definition.start must reference a start node")]
    MissingStartKind,
    #[error("workflow must include an end node")]
    MissingEndNode,
    #[error("workflow node '{node}' references missing node '{target}'")]
    MissingTransition { node: String, target: String },
    #[error("workflow node is invalid: {0}")]
    InvalidNode(String),
    #[error("workflow node '{0}' of kind action requires action configuration")]
    MissingAction(String),
    #[error("workflow node '{0}' retry.max_attempts must be greater than zero")]
    InvalidRetry(String),
    #[error("workflow node '{0}' timeout_seconds must be greater than zero")]
    InvalidTimeout(String),
    #[error("workflow node '{0}' max_iterations must be greater than zero")]
    InvalidLoopLimit(String),
    #[error(
        "workflow node '{0}' reentry.max_visits must be greater than zero when reentry is enabled"
    )]
    InvalidReentry(String),
    #[error("workflow node '{0}' uses unsupported local $ref cycle")]
    RefCycle(String),
    #[error("workflow $ref '{0}' could not be resolved")]
    MissingRef(String),
    #[error("runtime value reference '{0}' is invalid")]
    InvalidValueRef(String),
    #[error("declarative condition is invalid: {0}")]
    InvalidCondition(String),
    #[error("workflow node '{node}' parameters are invalid: {message}")]
    InvalidNodeParameters { node: String, message: String },
}
