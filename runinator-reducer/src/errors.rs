use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the reducer and workflow engine (RUNI1xx).

pub const WORKFLOW_NOT_FOUND: ErrorDescriptor =
    ErrorDescriptor::new("RUNI101", "workflow.not_found", "Workflow not found");
pub const WORKFLOW_RUN_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI102",
    "workflow_run.not_found",
    "Workflow run not found",
);
pub const WORKFLOW_RUN_SNAPSHOT_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI103",
    "workflow_run.snapshot_missing",
    "Workflow run is missing its workflow snapshot",
);
pub const WORKFLOW_TRIGGER_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI104",
    "workflow_trigger.not_found",
    "Workflow trigger not found",
);
pub const ACTION_CONFIG_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI105",
    "workflow.node.action_missing",
    "Action node has no action configuration",
);
pub const READY_NODE_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI106",
    "workflow.ready_node.not_found",
    "Ready node not found",
);
pub const READY_NODE_NOT_CLAIMED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI107",
    "workflow.ready_node.not_claimed",
    "Ready node is not claimed by this scheduler",
);

pub const SUBFLOW_RUN_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI110",
    "workflow.subflow.run_missing",
    "Subflow run not found",
);
pub const SUBFLOW_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI111",
    "workflow.subflow.missing",
    "Subflow workflow not found",
);
pub const SUBFLOW_MISSING_ID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI112",
    "workflow.subflow.missing_id",
    "Subflow workflow has no id",
);
pub const SUBFLOW_TARGET_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI113",
    "workflow.subflow.target_missing",
    "Subflow node is missing a target",
);
pub const SUBFLOW_INVALID_ID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI114",
    "workflow.subflow.invalid_id",
    "Subflow references a non-existent workflow id",
);

pub const COMPUTE_NODE_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI142",
    "workflow.compute.node_failed",
    "In-process compute node failed to evaluate",
);

pub const DELIVERABLE_SOURCE_UNRESOLVED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI143",
    "workflow.deliverable.source_unresolved",
    "Deliverable source did not resolve to an artifact",
);
pub const FOREIGN_LANGUAGE_CONFIG_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI144",
    "workflow.foreign_language.config_missing",
    "Foreign language runtime config is missing",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    WORKFLOW_NOT_FOUND,
    WORKFLOW_RUN_NOT_FOUND,
    WORKFLOW_RUN_SNAPSHOT_MISSING,
    WORKFLOW_TRIGGER_NOT_FOUND,
    ACTION_CONFIG_MISSING,
    READY_NODE_NOT_FOUND,
    READY_NODE_NOT_CLAIMED,
    SUBFLOW_RUN_MISSING,
    SUBFLOW_MISSING,
    SUBFLOW_MISSING_ID,
    SUBFLOW_TARGET_MISSING,
    SUBFLOW_INVALID_ID,
    COMPUTE_NODE_FAILED,
    DELIVERABLE_SOURCE_UNRESOLVED,
    FOREIGN_LANGUAGE_CONFIG_MISSING,
];

/// reducer error dictionary.
pub struct ReducerErrors;

impl EngineErrors for ReducerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
