use runinator_models::errors::{EngineErrors, ErrorDescriptor};

// numbered error dictionary for the database engine (RUNI5xx).

pub const ACTION_DISPATCH_INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI501",
    "database.action_dispatch.invalid_command_json",
    "Stored action dispatch command is invalid JSON",
);
pub const ORCHESTRATION_EVENT_INVALID_ID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI502",
    "database.orchestration_event.invalid_event_id",
    "Stored orchestration event id is invalid",
);
pub const READY_NODE_INVALID_SOURCE_EVENT_ID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI503",
    "database.ready_node.invalid_source_event_id",
    "Stored ready-node source event id is invalid",
);

pub const TRIGGER_MISSING_ID: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI504",
    "database.trigger.missing_id",
    "Workflow trigger row has no id",
);
pub const TRIGGER_NOT_FOUND: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI505",
    "database.trigger.not_found",
    "Workflow trigger not found",
);
pub const TRIGGER_NOT_CRON: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI506",
    "database.trigger.not_cron",
    "Workflow trigger has no cron schedule to replay",
);

// packaged functions.
pub const FUNCTION_ARTIFACT_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI507",
    "database.function.artifact_missing",
    "Function artifact not found",
);
/// deleting an artifact a published version still pins would make that version unrunnable.
pub const FUNCTION_ARTIFACT_IN_USE: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI508",
    "database.function.artifact_in_use",
    "Function artifact is still referenced by a version",
);
pub const FUNCTION_VERSION_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI509",
    "database.function.version_missing",
    "Function version not found",
);
pub const FUNCTION_ALIAS_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI510",
    "database.function.alias_missing",
    "Function alias not found",
);
pub const FUNCTION_ADAPTER_MISSING: ErrorDescriptor = ErrorDescriptor::new(
    "RUNI511",
    "database.function.adapter_missing",
    "Function adapter workflow not found",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    ACTION_DISPATCH_INVALID_JSON,
    ORCHESTRATION_EVENT_INVALID_ID,
    READY_NODE_INVALID_SOURCE_EVENT_ID,
    TRIGGER_MISSING_ID,
    TRIGGER_NOT_FOUND,
    TRIGGER_NOT_CRON,
    FUNCTION_ARTIFACT_MISSING,
    FUNCTION_ARTIFACT_IN_USE,
    FUNCTION_VERSION_MISSING,
    FUNCTION_ALIAS_MISSING,
    FUNCTION_ADAPTER_MISSING,
];

/// database engine error dictionary.
pub struct DatabaseErrors;

impl EngineErrors for DatabaseErrors {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
