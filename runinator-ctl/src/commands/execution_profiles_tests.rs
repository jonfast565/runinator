//! covers generic execution-profile request construction from command-line fields.

use super::*;
use runinator_models::execution_profiles::{ExecutionProfileCommand, ExecutionProfileSource};

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
