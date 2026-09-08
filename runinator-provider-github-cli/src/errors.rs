use runinator_models::errors::{ErrorDescriptor, ProviderErrors};

use crate::GitHubCliProvider;

pub const INVALID_PARAMS: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI001",
    "github_cli.invalid_params",
    "Invalid GitHub CLI parameters",
);
pub const PROFILE_REQUIRED: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI002",
    "github_cli.profile_required",
    "A GitHub execution profile is required",
);
pub const COMMAND_REJECTED: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI003",
    "github_cli.command_rejected",
    "GitHub CLI command is not allowed",
);
pub const COMMAND_START: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI004",
    "github_cli.command_start",
    "GitHub CLI could not start",
);
pub const COMMAND_FAILED: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI005",
    "github_cli.command_failed",
    "GitHub CLI command failed",
);
pub const COMMAND_TIMEOUT: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI006",
    "github_cli.command_timeout",
    "GitHub CLI command timed out",
);
pub const COMMAND_CANCELED: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI007",
    "github_cli.command_canceled",
    "GitHub CLI command was canceled",
);
pub const OUTPUT_TOO_LARGE: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI008",
    "github_cli.output_too_large",
    "GitHub CLI output exceeded the limit",
);
pub const INVALID_JSON: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI009",
    "github_cli.invalid_json",
    "GitHub CLI output was not valid JSON",
);
pub const UNSUPPORTED_ACTION: ErrorDescriptor = ErrorDescriptor::new(
    "GITHUBCLI010",
    "github_cli.unsupported_action",
    "Unsupported GitHub CLI action",
);

pub const DICTIONARY: &[ErrorDescriptor] = &[
    INVALID_PARAMS,
    PROFILE_REQUIRED,
    COMMAND_REJECTED,
    COMMAND_START,
    COMMAND_FAILED,
    COMMAND_TIMEOUT,
    COMMAND_CANCELED,
    OUTPUT_TOO_LARGE,
    INVALID_JSON,
    UNSUPPORTED_ACTION,
];

impl ProviderErrors for GitHubCliProvider {
    fn error_dictionary() -> &'static [ErrorDescriptor] {
        DICTIONARY
    }
}
