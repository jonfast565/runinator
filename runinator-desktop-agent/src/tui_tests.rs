//! execution-profile presentation in the terminal dashboard.

use super::*;

#[test]
fn selected_execution_profile_exposes_its_approval_controls_and_status() {
    let profile = crate::execution_profiles::LocalProfileStatus {
        id: uuid::Uuid::new_v4(),
        name: "local credentials".to_string(),
        config_digest: "abcdef0123456789".to_string(),
        enabled: true,
        approved: false,
        message: "local approval required".to_string(),
    };

    let details = execution_profile_details(&[profile], 0).join("\n");

    assert!(
        details.contains("Selected 1/1: local credentials"),
        "{details}"
    );
    assert!(
        details.contains("not approved on this computer"),
        "{details}"
    );
    assert!(details.contains("1-9 select directly"), "{details}");
    assert!(details.contains("a approve"), "{details}");
    assert!(details.contains("A approve all"), "{details}");
    assert!(details.contains("r revoke"), "{details}");
    assert!(details.contains("> 1. local credentials"), "{details}");
}

#[test]
fn numbered_command_selects_a_specific_execution_profile() {
    let mut profiles =
        ["first", "second", "third"].map(|name| crate::execution_profiles::LocalProfileStatus {
            id: uuid::Uuid::new_v4(),
            name: name.to_string(),
            config_digest: format!("{name}-digest"),
            enabled: true,
            approved: false,
            message: "local approval required".to_string(),
        });
    let mut selected = 0;

    let refresh = handle_profile_command(&mut profiles, &mut selected, ProfileCommand::Select(2));

    assert!(!refresh);
    assert_eq!(selected, 2);
    let details = execution_profile_details(&profiles, selected).join("\n");
    assert!(details.contains("Selected 3/3: third"), "{details}");
    assert!(details.contains("> 3. third"), "{details}");
}

#[test]
fn bulk_approval_updates_every_enabled_profile_in_one_config() {
    let already_approved = crate::execution_profiles::LocalProfileStatus {
        id: uuid::Uuid::new_v4(),
        name: "existing".to_string(),
        config_digest: "existing-digest".to_string(),
        enabled: true,
        approved: true,
        message: "ready".to_string(),
    };
    let pending = crate::execution_profiles::LocalProfileStatus {
        id: uuid::Uuid::new_v4(),
        name: "pending".to_string(),
        config_digest: "pending-digest".to_string(),
        enabled: true,
        approved: false,
        message: "local approval required".to_string(),
    };
    let changed = crate::execution_profiles::LocalProfileStatus {
        id: uuid::Uuid::new_v4(),
        name: "changed".to_string(),
        config_digest: "new-digest".to_string(),
        enabled: true,
        approved: false,
        message: "local approval required".to_string(),
    };
    let disabled = crate::execution_profiles::LocalProfileStatus {
        id: uuid::Uuid::new_v4(),
        name: "disabled".to_string(),
        config_digest: "disabled-digest".to_string(),
        enabled: false,
        approved: false,
        message: "disabled".to_string(),
    };
    let mut config = AgentConfig::default();
    config
        .approved_execution_profiles
        .insert(already_approved.id, already_approved.config_digest.clone());
    config
        .approved_execution_profiles
        .insert(changed.id, "old-digest".to_string());

    let approved = approve_enabled_profiles(
        &mut config,
        &[
            already_approved.clone(),
            pending.clone(),
            changed.clone(),
            disabled.clone(),
        ],
    );

    assert_eq!(approved, ["pending", "changed"]);
    assert_eq!(
        config.approved_execution_profiles.get(&pending.id),
        Some(&pending.config_digest)
    );
    assert_eq!(
        config.approved_execution_profiles.get(&changed.id),
        Some(&changed.config_digest)
    );
    assert!(
        !config
            .approved_execution_profiles
            .contains_key(&disabled.id)
    );
}
