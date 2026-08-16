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
pub const IMPORT_UNKNOWN_PIPELINE_MEMBER: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI123",
    "workflow.import.unknown_pipeline_member",
    "Imported pipeline references an unknown member workflow",
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
/// a debug verb named a thread of control the run does not hold.
pub const DEBUG_CURSOR_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI147",
    "workflow.debug.cursor_not_found",
    "Workflow run has no such cursor",
);

/// a speculative fork could not be created: no such parent, no such entry node, or the run already
/// carries as many "what if" branches as its state blob may hold.
pub const DEBUG_FORK_INVALID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI148",
    "workflow.debug.fork_invalid",
    "Cannot fork a speculative branch here",
);

/// an operation valid only on a speculative branch was aimed at a real thread of control.
pub const DEBUG_SPECULATIVE_ONLY: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI149",
    "workflow.debug.speculative_only",
    "Only a speculative cursor supports this operation",
);

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
pub const MUTEX_MIGRATION_CONFLICT: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI178",
    "workflow.mutex.migration_conflict",
    "Workflow mutex migration found conflicting live ownership",
);

// notification policy emission and delivery.
pub const NOTIFY_UNROUTABLE_CHANNEL: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI142",
    "notification.policy.unroutable_channel",
    "Notification policy targets a channel with no delivery provider",
);
pub const NOTIFY_MISSING_TARGET: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI143",
    "notification.policy.missing_target",
    "Notification policy has an external channel but no target",
);

pub const FREEZE_WINDOW_INVALID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI144",
    "schedule.freeze_window.invalid",
    "Freeze window is not a usable range",
);
pub const FREEZE_WINDOW_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI145",
    "schedule.freeze_window.not_found",
    "Freeze window not found",
);
pub const BACKFILL_INVALID_RANGE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI146",
    "schedule.backfill.invalid_range",
    "Backfill range is not replayable",
);

// artifact storage.
/// artifact bytes could not be written to the object store.
pub const ARTIFACT_STORE_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI150",
    "artifact.store_failed",
    "Could not store artifact bytes",
);
/// artifact bytes could not be read back. for a pre-blob row this usually means the upload landed on
/// a different replica's filesystem, which is exactly what the object store removes.
pub const ARTIFACT_UNREADABLE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI151",
    "artifact.unreadable",
    "Could not read artifact bytes",
);

// packaged functions.
/// an artifact digest was not a well-formed `sha256:<hex>`. checked before the digest is used to
/// build an object key, which is what keeps it from being a path.
pub const FUNCTION_INVALID_DIGEST: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI160",
    "function.invalid_digest",
    "Artifact digest is malformed",
);
/// uploaded bytes hashed to something other than the digest they were uploaded under. the whole
/// pinning story rests on the digest naming these exact bytes, so this is refused, not corrected.
pub const FUNCTION_DIGEST_MISMATCH: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI161",
    "function.digest_mismatch",
    "Artifact bytes do not match their digest",
);
/// a publish named an artifact that has not been uploaded.
pub const FUNCTION_ARTIFACT_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI162",
    "function.artifact_missing",
    "Artifact has not been uploaded",
);
/// artifact bytes could not be written to or read from the object store.
pub const FUNCTION_ARTIFACT_STORAGE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI163",
    "function.artifact_storage",
    "Could not store or read artifact bytes",
);
/// a package, version, export, or alias was not found.
pub const FUNCTION_NOT_FOUND: ErrorDescriptor =
    ErrorDescriptor::new("RUNI164", "function.not_found", "Function not found");

// the wdl console.
/// a console session was not found.
pub const CONSOLE_SESSION_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI170",
    "console.session_not_found",
    "Console session not found",
);
/// a console cell was not found.
pub const CONSOLE_CELL_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI171",
    "console.cell_not_found",
    "Console cell not found",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    IMPORT_UNKNOWN_SUBFLOW,
    IMPORT_INVALID_TRIGGER_BLACKOUT,
    IMPORT_UNKNOWN_CHAINED_TARGET,
    IMPORT_UNKNOWN_PIPELINE_MEMBER,
    DEBUG_NOT_FOUND,
    DEBUG_DISABLED,
    DEBUG_TERMINAL,
    DEBUG_NO_ACTIVE_NODE,
    DEBUG_CURSOR_NOT_FOUND,
    DEBUG_FORK_INVALID,
    DEBUG_SPECULATIVE_ONLY,
    DEBUG_INVALID_PATCH,
    PAUSE_NOT_FOUND,
    RESUME_NOT_FOUND,
    CANCEL_NOT_FOUND,
    CONTROL_PUBLISH,
    REPLAY_NOT_FOUND,
    REPLAY_MISSING_STEP,
    REPLAY_CONTROL_FLOW,
    NOTIFY_UNROUTABLE_CHANNEL,
    NOTIFY_MISSING_TARGET,
    FREEZE_WINDOW_INVALID,
    FREEZE_WINDOW_NOT_FOUND,
    BACKFILL_INVALID_RANGE,
    BACKGROUND_LOOP_EXITED,
    MUTEX_MIGRATION_CONFLICT,
    ARTIFACT_STORE_FAILED,
    ARTIFACT_UNREADABLE,
    FUNCTION_INVALID_DIGEST,
    FUNCTION_DIGEST_MISMATCH,
    FUNCTION_ARTIFACT_MISSING,
    FUNCTION_ARTIFACT_STORAGE,
    FUNCTION_NOT_FOUND,
    CONSOLE_SESSION_NOT_FOUND,
    CONSOLE_CELL_NOT_FOUND,
];

/// orchestration engine error dictionary.
pub struct EngineErrorCatalog;

impl EngineErrors for EngineErrorCatalog {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
