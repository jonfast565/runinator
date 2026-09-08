use super::*;

#[test]
fn metadata_requires_a_github_execution_profile() {
    let metadata = GitHubCliProvider.metadata();
    assert_eq!(metadata.name, "github_cli");
    assert_eq!(metadata.metadata.credential_scopes, ["github"]);
    assert_eq!(
        metadata.metadata.execution_profile,
        ExecutionProfileSupport::Subprocess
    );
    assert!(
        metadata
            .actions
            .iter()
            .any(|action| action.function_name == "api")
    );
    assert!(
        metadata
            .actions
            .iter()
            .any(|action| action.function_name == "graphql")
    );
    assert!(
        metadata
            .actions
            .iter()
            .any(|action| action.function_name == "run")
    );
}

#[test]
fn auth_and_extension_commands_are_not_allowlisted() {
    assert!(!ALLOWED_COMMANDS.contains(&"auth"));
    assert!(!ALLOWED_COMMANDS.contains(&"config"));
    assert!(!ALLOWED_COMMANDS.contains(&"alias"));
    assert!(!ALLOWED_COMMANDS.contains(&"extension"));
}
