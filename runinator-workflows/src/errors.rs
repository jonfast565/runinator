use runinator_models::errors::{EngineErrors, ErrorDescriptor};
use thiserror::Error;

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
pub struct WorkflowTypeDiagnostic {
    pub path: String,
    pub expected: String,
    pub actual: String,
    pub message: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkflowValidationError {
    #[error("WORKFLOW001 - workflow definition.nodes must be an array")]
    MissingNodes,
    #[error("WORKFLOW002 - workflow definition.start must name the first node")]
    MissingStart,
    #[error("WORKFLOW003 - workflow node '{0}' is duplicated")]
    DuplicateNode(String),
    #[error("WORKFLOW004 - workflow start node '{0}' does not exist")]
    MissingStartNode(String),
    #[error("WORKFLOW005 - workflow definition.start must reference a start node")]
    MissingStartKind,
    #[error("WORKFLOW006 - workflow must include an end node")]
    MissingEndNode,
    #[error("WORKFLOW007 - workflow node '{node}' references missing node '{target}'")]
    MissingTransition { node: String, target: String },
    #[error("WORKFLOW008 - workflow node is invalid: {0}")]
    InvalidNode(String),
    #[error("WORKFLOW009 - workflow node '{0}' of kind action requires action configuration")]
    MissingAction(String),
    #[error("WORKFLOW010 - workflow node '{0}' retry.max_attempts must be greater than zero")]
    InvalidRetry(String),
    #[error("WORKFLOW011 - workflow node '{0}' timeout_seconds must be greater than zero")]
    InvalidTimeout(String),
    #[error("WORKFLOW012 - workflow node '{0}' max_iterations must be greater than zero")]
    InvalidLoopLimit(String),
    #[error(
        "WORKFLOW013 - workflow node '{0}' of kind subflow requires subflow_id or subflow.workflow_name"
    )]
    MissingSubflowTarget(String),
    #[error(
        "WORKFLOW014 - workflow node '{0}' reentry.max_visits must be greater than zero when reentry is enabled"
    )]
    InvalidReentry(String),
    #[error("WORKFLOW015 - workflow node '{0}' uses unsupported local $ref cycle")]
    RefCycle(String),
    #[error("WORKFLOW016 - workflow $ref '{0}' could not be resolved")]
    MissingRef(String),
    #[error("WORKFLOW017 - runtime value reference '{0}' is invalid")]
    InvalidValueRef(String),
    #[error("WORKFLOW018 - declarative condition is invalid: {0}")]
    InvalidCondition(String),
    #[error("WORKFLOW019 - workflow node '{node}' parameters are invalid: {message}")]
    InvalidNodeParameters { node: String, message: String },
    #[error("WORKFLOW020 - workflow node '{node}' references non-existent workflow with id {id}")]
    InvalidSubflowId { node: String, id: i64 },
    #[error("WORKFLOW021 - workflow type validation failed: {0}")]
    TypeError(String),
    #[error("WORKFLOW022 - workflow type validation failed: {}", .0.message)]
    TypeDiagnostic(WorkflowTypeDiagnostic),
}

// numbered error dictionary for the workflow validator.
pub const MISSING_NODES: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW001",
    "workflow.missing_nodes",
    "definition.nodes must be an array",
);
pub const MISSING_START: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW002",
    "workflow.missing_start",
    "definition.start must name the first node",
);
pub const DUPLICATE_NODE: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW003",
    "workflow.duplicate_node",
    "Node is duplicated",
);
pub const MISSING_START_NODE: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW004",
    "workflow.missing_start_node",
    "Start node does not exist",
);
pub const MISSING_START_KIND: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW005",
    "workflow.missing_start_kind",
    "definition.start must reference a start node",
);
pub const MISSING_END_NODE: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW006",
    "workflow.missing_end_node",
    "Workflow must include an end node",
);
pub const MISSING_TRANSITION: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW007",
    "workflow.missing_transition",
    "Node references a missing node",
);
pub const INVALID_NODE: ErrorDescriptor =
    ErrorDescriptor::new("WORKFLOW008", "workflow.invalid_node", "Node is invalid");
pub const MISSING_ACTION: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW009",
    "workflow.missing_action",
    "Action node requires action configuration",
);
pub const INVALID_RETRY: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW010",
    "workflow.invalid_retry",
    "retry.max_attempts must be greater than zero",
);
pub const INVALID_TIMEOUT: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW011",
    "workflow.invalid_timeout",
    "timeout_seconds must be greater than zero",
);
pub const INVALID_LOOP_LIMIT: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW012",
    "workflow.invalid_loop_limit",
    "max_iterations must be greater than zero",
);
pub const MISSING_SUBFLOW_TARGET: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW013",
    "workflow.missing_subflow_target",
    "Subflow requires subflow_id or subflow.workflow_name",
);
pub const INVALID_REENTRY: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW014",
    "workflow.invalid_reentry",
    "reentry.max_visits must be greater than zero when reentry is enabled",
);
pub const REF_CYCLE: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW015",
    "workflow.ref_cycle",
    "Unsupported local $ref cycle",
);
pub const MISSING_REF: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW016",
    "workflow.missing_ref",
    "$ref could not be resolved",
);
pub const INVALID_VALUE_REF: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW017",
    "workflow.invalid_value_ref",
    "Runtime value reference is invalid",
);
pub const INVALID_CONDITION: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW018",
    "workflow.invalid_condition",
    "Declarative condition is invalid",
);
pub const INVALID_NODE_PARAMETERS: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW019",
    "workflow.invalid_node_parameters",
    "Node parameters are invalid",
);
pub const INVALID_SUBFLOW_ID: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW020",
    "workflow.invalid_subflow_id",
    "References a non-existent workflow id",
);
pub const TYPE_ERROR: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW021",
    "workflow.type_error",
    "Type validation failed",
);
pub const TYPE_DIAGNOSTIC: ErrorDescriptor = ErrorDescriptor::new(
    "WORKFLOW022",
    "workflow.type_diagnostic",
    "Type validation failed",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    MISSING_NODES,
    MISSING_START,
    DUPLICATE_NODE,
    MISSING_START_NODE,
    MISSING_START_KIND,
    MISSING_END_NODE,
    MISSING_TRANSITION,
    INVALID_NODE,
    MISSING_ACTION,
    INVALID_RETRY,
    INVALID_TIMEOUT,
    INVALID_LOOP_LIMIT,
    MISSING_SUBFLOW_TARGET,
    INVALID_REENTRY,
    REF_CYCLE,
    MISSING_REF,
    INVALID_VALUE_REF,
    INVALID_CONDITION,
    INVALID_NODE_PARAMETERS,
    INVALID_SUBFLOW_ID,
    TYPE_ERROR,
    TYPE_DIAGNOSTIC,
];

impl EngineErrors for WorkflowValidationError {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}

impl WorkflowValidationError {
    pub fn type_diagnostic(&self) -> Option<&WorkflowTypeDiagnostic> {
        match self {
            Self::TypeDiagnostic(diagnostic) => Some(diagnostic),
            _ => None,
        }
    }
}
