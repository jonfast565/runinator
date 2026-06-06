use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::provider::AiCommandProvider;

// numbered error dictionary for the ai-command provider. the dotted `key` stays
// the runtime error code; the message renders as "AI00N - <summary>: <detail>".
pub(crate) const INVALID_PARAMS: ErrorDescriptor =
    ErrorDescriptor::new("AI001", "ai_command.invalid_params", "Invalid parameters");
pub(crate) const CANCELED: ErrorDescriptor =
    ErrorDescriptor::new("AI002", "ai_command.canceled", "Command canceled");
pub(crate) const TIMEOUT: ErrorDescriptor =
    ErrorDescriptor::new("AI003", "ai_command.timeout", "Command timed out");
pub(crate) const NONZERO_EXIT: ErrorDescriptor = ErrorDescriptor::new(
    "AI004",
    "ai_command.nonzero_exit",
    "Command exited with a non-zero status",
);
pub(crate) const INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "AI005",
    "ai_command.invalid_json",
    "Command output was not valid JSON",
);
pub(crate) const CLAUDE_CANCELED: ErrorDescriptor = ErrorDescriptor::new(
    "AI006",
    "ai_command.claude_code.canceled",
    "Claude Code command canceled",
);
pub(crate) const CLAUDE_SPAWN: ErrorDescriptor = ErrorDescriptor::new(
    "AI007",
    "ai_command.claude_code.spawn",
    "Failed to spawn Claude Code",
);
pub(crate) const CLAUDE_TIMEOUT: ErrorDescriptor = ErrorDescriptor::new(
    "AI008",
    "ai_command.claude_code.timeout",
    "Claude Code timed out",
);
pub(crate) const CLAUDE_EXIT_CODE: ErrorDescriptor = ErrorDescriptor::new(
    "AI009",
    "ai_command.claude_code.exit_code",
    "Claude Code exited with a non-zero status",
);
pub(crate) const CLAUDE_INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "AI010",
    "ai_command.claude_code.invalid_json",
    "Claude Code output was not valid JSON",
);

pub(crate) const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    CANCELED,
    TIMEOUT,
    NONZERO_EXIT,
    INVALID_JSON,
    CLAUDE_CANCELED,
    CLAUDE_SPAWN,
    CLAUDE_TIMEOUT,
    CLAUDE_EXIT_CODE,
    CLAUDE_INVALID_JSON,
];

impl ProviderErrors for AiCommandProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
