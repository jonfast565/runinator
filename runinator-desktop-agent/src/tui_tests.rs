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

    assert!(details.contains("1/1: local credentials"), "{details}");
    assert!(
        details.contains("not approved on this computer"),
        "{details}"
    );
    assert!(details.contains("a approve selected profile"), "{details}");
    assert!(details.contains("r revoke selected profile"), "{details}");
}
