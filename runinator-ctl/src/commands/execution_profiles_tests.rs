//! covers generic execution-profile request construction from command-line fields.

use super::*;
use chrono::Utc;
use runinator_models::execution_profiles::{
    ExecutionProfileAgentStatus, ExecutionProfileApprovalState, ExecutionProfileCommand,
    ExecutionProfileHealth, ExecutionProfileSource,
};
use uuid::Uuid;

#[test]
fn request_reads_collection_and_exposure_json() {
    let profile = profile_request(
        "desktop-session",
        Some("desktop credentials"),
        &["provider-a".into(), "provider-b".into()],
        r#"{
            "probe": { "argv": ["credential-tool", "status"] },
            "sources": [{ "type": "file", "path": "~/.credentials", "target": ".credentials" }]
        }"#,
        Some(
            r#"{ "home_overlay": true, "environment": { "CREDENTIAL_HOME": "${PROFILE_HOME}" } }"#,
        ),
    )
    .expect("valid profile request");

    assert_eq!(profile.name, "desktop-session");
    assert_eq!(profile.description, "desktop credentials");
    assert_eq!(profile.credential_scopes, ["provider-a", "provider-b"]);
    assert_eq!(
        profile.collection.probe,
        Some(ExecutionProfileCommand {
            argv: vec!["credential-tool".into(), "status".into()],
            interactive: false,
        })
    );
    assert_eq!(
        profile.collection.sources,
        [ExecutionProfileSource::File {
            path: "~/.credentials".into(),
            target: ".credentials".into(),
        }]
    );
    assert!(profile.exposure.home_overlay);
    assert_eq!(
        profile.exposure.environment.get("CREDENTIAL_HOME"),
        Some(&"${PROFILE_HOME}".to_string())
    );
}

#[test]
fn request_defaults_exposure_and_names_invalid_json() {
    let profile = profile_request(
        "desktop-session",
        None,
        &["provider-a".into()],
        r#"{ "sources": [{ "type": "file", "path": "~/.credentials", "target": ".credentials" }] }"#,
        None,
    )
    .expect("valid profile request");
    assert_eq!(profile.exposure, ExecutionProfileExposureSpec::default());

    let error = profile_request("desktop-session", None, &["provider-a".into()], "{", None)
        .expect_err("invalid collection json");
    assert!(error.to_string().contains("--collection"));
}

#[test]
fn collection_status_text_reports_desktop_approval_and_error() {
    let profile_id = Uuid::now_v7();
    let agent_id = Uuid::now_v7();
    let now = Utc::now();
    let status = ExecutionProfileCollectionStatus {
        profile_id,
        config_digest: "digest".into(),
        publication_health: ExecutionProfileHealth::Ready,
        current_revision: Some(3),
        published_at: Some(now),
        expires_at: None,
        latest_operation: None,
        agents: vec![ExecutionProfileAgentStatus {
            profile_id,
            agent_id,
            config_digest: "digest".into(),
            approval: ExecutionProfileApprovalState::Approved,
            last_seen_at: now,
            last_attempt_at: Some(now),
            last_success_at: Some(now),
            last_error: Some("collection failed after the last success".into()),
        }],
    };
    let names = BTreeMap::from([(profile_id, "desktop-session".into())]);

    let text = collection_status_text(&[status], &names);

    assert!(text.contains("desktop-session"));
    assert!(text.contains("publication: ready; revision: 3"));
    assert!(text.contains(&agent_id.to_string()));
    assert!(text.contains("approved"));
    assert!(text.contains("collection failed after the last success"));
}
