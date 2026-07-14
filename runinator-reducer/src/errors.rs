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

pub const CHAIN_TARGET_UNRESOLVED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI115",
    "workflow.chain.target_unresolved",
    "Chained trigger target workflow not found",
);
pub const CHAIN_DEPTH_EXCEEDED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI116",
    "workflow.chain.depth_exceeded",
    "Chained workflow depth limit exceeded",
);

pub const COMPUTE_NODE_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI142",
    "workflow.compute.node_failed",
    "In-process compute node failed to evaluate",
);

pub const ARTIFACT_SOURCE_UNRESOLVED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI143",
    "workflow.output.artifact_source_unresolved",
    "Output artifact source did not resolve to an artifact",
);
pub const FOREIGN_LANGUAGE_CONFIG_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI144",
    "workflow.foreign_language.config_missing",
    "Foreign language runtime config is missing",
);

pub const ASSERT_PARAMS_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI150",
    "workflow.assert.params_missing",
    "Assert node is missing an assertions parameter",
);
pub const TRANSFORM_PARAMS_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI151",
    "workflow.transform.params_missing",
    "Transform node is missing a bindings parameter",
);
pub const MUTEX_NAME_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI152",
    "workflow.mutex.name_missing",
    "Mutex node is missing a name parameter",
);
pub const THROTTLE_NAME_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI153",
    "workflow.throttle.name_missing",
    "Throttle node is missing a name parameter",
);
pub const AWAIT_RUN_IDS_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI154",
    "workflow.await_run.run_ids_missing",
    "AwaitRun node is missing a run_ids parameter",
);
pub const DEBOUNCE_DELAY_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI155",
    "workflow.debounce.delay_missing",
    "Debounce node is missing a delay_seconds parameter",
);
pub const COLLECT_NAME_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI156",
    "workflow.collect.name_missing",
    "Collect node is missing a name parameter",
);
pub const BARRIER_NAME_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI157",
    "workflow.barrier.name_missing",
    "Barrier node is missing a name parameter",
);
pub const CIRCUIT_BREAKER_NAME_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI158",
    "workflow.circuit_breaker.name_missing",
    "CircuitBreaker node is missing a name parameter",
);
pub const EVENT_SOURCE_TYPE_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI159",
    "workflow.event_source.type_missing",
    "EventSource node is missing an event_type parameter",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    ASSERT_PARAMS_MISSING,
    TRANSFORM_PARAMS_MISSING,
    MUTEX_NAME_MISSING,
    THROTTLE_NAME_MISSING,
    AWAIT_RUN_IDS_MISSING,
    DEBOUNCE_DELAY_MISSING,
    COLLECT_NAME_MISSING,
    BARRIER_NAME_MISSING,
    CIRCUIT_BREAKER_NAME_MISSING,
    EVENT_SOURCE_TYPE_MISSING,
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
    CHAIN_TARGET_UNRESOLVED,
    CHAIN_DEPTH_EXCEEDED,
    COMPUTE_NODE_FAILED,
    ARTIFACT_SOURCE_UNRESOLVED,
    FOREIGN_LANGUAGE_CONFIG_MISSING,
];

/// reducer error dictionary.
pub struct ReducerErrors;

impl EngineErrors for ReducerErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
