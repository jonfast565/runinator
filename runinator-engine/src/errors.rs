use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the orchestration engine. these codes were previously owned by
// runinator-ws (RUNI1xx) and moved here with the repository/loop code that emits them; the numbers
// are kept stable so existing logs and dashboards keep resolving.

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
pub const IMPORT_UNKNOWN_CHAINED_TARGET: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI122",
    "workflow.import.unknown_chained_target",
    "Imported workflow chains to an unknown target workflow",
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
pub const BACKGROUND_LOOP_EXITED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI177",
    "ws.background.loop_exited",
    "A background orchestration loop exited unexpectedly",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    IMPORT_UNKNOWN_SUBFLOW,
    IMPORT_INVALID_TRIGGER_BLACKOUT,
    IMPORT_UNKNOWN_CHAINED_TARGET,
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
    BACKGROUND_LOOP_EXITED,
];

/// orchestration engine error dictionary.
pub struct EngineErrorCatalog;

impl EngineErrors for EngineErrorCatalog {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
