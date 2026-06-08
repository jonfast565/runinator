use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the web service engine (RUNI1xx).

// workflow and run lookups.
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

// subflow resolution.
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

// pack import validation.
pub const IMPORT_UNKNOWN_SUBFLOW: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI120",
    "workflow.import.unknown_subflow",
    "Imported workflow references an unknown subflow",
);
pub const IMPORT_INVALID_TRIGGER_BLACKOUT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI121",
    "workflow.import.invalid_trigger_blackout",
    "Trigger blackout datetime is invalid",
);

// debug, control, and replay.
pub const DEBUG_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI130",
    "workflow.debug.not_found",
    "Workflow run not found",
);
pub const DEBUG_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI131",
    "workflow.debug.disabled",
    "Workflow run is not a debug run",
);
pub const DEBUG_TERMINAL: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI132",
    "workflow.debug.terminal",
    "Workflow run is terminal",
);
pub const DEBUG_NO_ACTIVE_NODE: ErrorDescriptor =
    ErrorDescriptor::new("RUNI133", "workflow.debug.no_active_node", "No active node");
pub const DEBUG_INVALID_PATCH: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI134",
    "workflow.debug.invalid_patch",
    "Invalid debug patch",
);
pub const PAUSE_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI135",
    "workflow.pause.not_found",
    "Workflow run not found",
);
pub const RESUME_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI136",
    "workflow.resume.not_found",
    "Workflow run not found",
);
pub const CANCEL_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI137",
    "workflow.cancel.not_found",
    "Workflow run not found",
);
pub const CONTROL_PUBLISH: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI138",
    "workflow.control.publish",
    "Failed to publish control command",
);
pub const REPLAY_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI139",
    "workflow.replay.not_found",
    "Workflow run not found",
);
pub const REPLAY_MISSING_STEP: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI140",
    "workflow.replay.missing_step",
    "Step not found in workflow snapshot",
);
pub const REPLAY_CONTROL_FLOW: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI141",
    "workflow.replay.control_flow",
    "Cannot replay through a control-flow ancestor",
);
pub const COMPUTE_NODE_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI142",
    "workflow.compute.node_failed",
    "In-process compute node failed to evaluate",
);

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
pub const BROKER_KAFKA_FEATURE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI175",
    "ws.broker.kafka_feature_disabled",
    "Kafka broker support not compiled in",
);
pub const BROKER_RABBITMQ: ErrorDescriptor =
    ErrorDescriptor::new("RUNI176", "ws.broker.rabbitmq", "RabbitMQ broker error");
pub const BROKER_RABBITMQ_FEATURE_DISABLED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI177",
    "ws.broker.rabbitmq_feature_disabled",
    "RabbitMQ broker support not compiled in",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    WORKFLOW_NOT_FOUND,
    WORKFLOW_RUN_NOT_FOUND,
    WORKFLOW_RUN_SNAPSHOT_MISSING,
    WORKFLOW_TRIGGER_NOT_FOUND,
    ACTION_CONFIG_MISSING,
    COMPUTE_NODE_FAILED,
    READY_NODE_NOT_FOUND,
    READY_NODE_NOT_CLAIMED,
    SUBFLOW_RUN_MISSING,
    SUBFLOW_MISSING,
    SUBFLOW_MISSING_ID,
    SUBFLOW_TARGET_MISSING,
    SUBFLOW_INVALID_ID,
    IMPORT_UNKNOWN_SUBFLOW,
    IMPORT_INVALID_TRIGGER_BLACKOUT,
    DEBUG_NOT_FOUND,
    DEBUG_DISABLED,
    DEBUG_TERMINAL,
    DEBUG_NO_ACTIVE_NODE,
    DEBUG_INVALID_PATCH,
    PAUSE_NOT_FOUND,
    RESUME_NOT_FOUND,
    CANCEL_NOT_FOUND,
    CONTROL_PUBLISH,
    REPLAY_NOT_FOUND,
    REPLAY_MISSING_STEP,
    REPLAY_CONTROL_FLOW,
    BROKER_WORKFLOW_RESULTS,
    BROKER_INVALID_ENDPOINT,
    BROKER_CLIENT,
    BROKER_UNKNOWN_BACKEND,
    BROKER_KAFKA,
    BROKER_KAFKA_FEATURE_DISABLED,
    BROKER_RABBITMQ,
    BROKER_RABBITMQ_FEATURE_DISABLED,
];

/// web service engine error dictionary.
pub struct WsErrors;

impl EngineErrors for WsErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
