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
pub(crate) const CLAUDE_INTERACTIVE_NOT_PERMITTED: ErrorDescriptor = ErrorDescriptor::new(
    "AI011",
    "ai_command.claude_code.interactive_not_permitted",
    "Interactive Claude Code is only available on a desktop worker agent",
);
pub(crate) const CLAUDE_INPUT: ErrorDescriptor = ErrorDescriptor::new(
    "AI012",
    "ai_command.claude_code.input",
    "Claude Code session input failed",
);
pub(crate) const CODEX_CANCELED: ErrorDescriptor = ErrorDescriptor::new(
    "AI013",
    "ai_command.codex.canceled",
    "Codex command canceled",
);
pub(crate) const CODEX_SPAWN: ErrorDescriptor =
    ErrorDescriptor::new("AI014", "ai_command.codex.spawn", "Failed to spawn Codex");
pub(crate) const CODEX_TIMEOUT: ErrorDescriptor = ErrorDescriptor::new(
    "AI015",
    "ai_command.codex.timeout",
    "Codex command timed out",
);
pub(crate) const CODEX_EXIT_CODE: ErrorDescriptor = ErrorDescriptor::new(
    "AI016",
    "ai_command.codex.exit_code",
    "Codex exited with a non-zero status",
);
pub(crate) const CODEX_PROTOCOL: ErrorDescriptor = ErrorDescriptor::new(
    "AI017",
    "ai_command.codex.protocol",
    "Codex protocol failed",
);
pub(crate) const CODEX_INPUT: ErrorDescriptor =
    ErrorDescriptor::new("AI018", "ai_command.codex.input", "Codex input is invalid");
pub(crate) const CLAUDE_SCHEMA: ErrorDescriptor = ErrorDescriptor::new(
    "AI019",
    "ai_command.claude_code.schema",
    "Claude Code structured output did not match its schema",
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
    CLAUDE_INTERACTIVE_NOT_PERMITTED,
    CLAUDE_INPUT,
    CODEX_CANCELED,
    CODEX_SPAWN,
    CODEX_TIMEOUT,
    CODEX_EXIT_CODE,
    CODEX_PROTOCOL,
    CODEX_INPUT,
    CLAUDE_SCHEMA,
];

impl<R> ProviderErrors for AiCommandProvider<R> {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
