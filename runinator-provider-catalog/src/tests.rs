use crate::{built_in_providers, metadata};

#[test]
fn built_in_provider_names_stay_in_runtime_registration_order() {
    let names = built_in_providers()
        .iter()
        .map(|provider| provider.name())
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        [
            "console",
            "aws",
            "sql",
            "jira",
            "github",
            "slack",
            "git",
            "ai-command",
            "approval",
            "email",
            "std",
        ]
    );
}

#[test]
fn metadata_matches_registered_provider_names() {
    let provider_names = built_in_providers()
        .iter()
        .map(|provider| provider.name())
        .collect::<Vec<_>>();
    let metadata_names = metadata()
        .into_iter()
        .map(|provider| provider.name)
        .collect::<Vec<_>>();

    assert_eq!(metadata_names, provider_names);
}
