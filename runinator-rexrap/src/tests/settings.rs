//! the `.rexraps` settings front end: secrets and config, and the values it refuses.

use super::*;

#[test]
fn parses_rexraps_secrets_and_config() {
    use crate::parse_secrets_str;
    use runinator_models::settings::SettingKind;

    let src = r#"
        secret jira.token = "abc123"
        secret jira.api.key = "xyz"
        config jira.base_url = "https://acme.atlassian.net"
        config app.retries = 3
        config app.flags = { beta: true, region: "us" }
    "#;
    let bundle = parse_secrets_str(src).expect("parse rexraps");
    assert_eq!(bundle.settings.len(), 5);

    let token = &bundle.settings[0];
    assert_eq!(token.scope, "jira");
    assert_eq!(token.name, "token");
    assert_eq!(token.kind, SettingKind::Secret);
    assert_eq!(token.value, Value::from("abc123"));

    // multi-segment names join with `/`, matching rexrap secret addressing.
    assert_eq!(bundle.settings[1].name, "api/key");

    let base = &bundle.settings[2];
    assert_eq!(base.kind, SettingKind::Config);
    assert_eq!(base.value, Value::from("https://acme.atlassian.net"));

    assert_eq!(bundle.settings[3].value, Value::from(3));

    let flags = &bundle.settings[4];
    assert_eq!(flags.kind, SettingKind::Config);
    assert_eq!(flags.value.get("beta"), Some(&Value::from(true)));
    assert_eq!(flags.value.get("region"), Some(&Value::from("us")));
}
#[test]
fn rejects_rexraps_reference_value() {
    use crate::parse_secrets_str;
    let err = parse_secrets_str("config app.url = config.other.url\n").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("literals"), "{message}");
}
#[test]
fn rejects_rexraps_interpolated_value() {
    use crate::parse_secrets_str;
    let err = parse_secrets_str("secret app.k = \"a-${params.x}\"\n").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("interpolate"), "{message}");
}
#[test]
fn rexraps_round_trips_through_export() {
    use crate::{parse_secrets_str, secrets_to_rexraps};
    let src = r#"
        secret jira.token = "abc123"
        config jira.base_url = "https://acme.atlassian.net"
        config app.flags = { beta: true }
        config app.tags = ["x", "y"]
    "#;
    let bundle = parse_secrets_str(src).expect("parse");
    let rendered = secrets_to_rexraps(&bundle);
    let reparsed = parse_secrets_str(&rendered).expect("reparse");
    assert_eq!(bundle.settings, reparsed.settings, "rendered:\n{rendered}");
}

#[test]
fn mixed_settings_profiles_schema_and_expiry_round_trip() {
    use crate::{parse_settings_str, settings_to_rexrap};

    let src = r#"
        @schema({ type: "integer", minimum: 1 })
        config app.retries = 3

        @expires_at("2026-12-01T00:00:00Z")
        secret github.token = "development-only-value"

        profile "github-default" = {
            description: "GitHub login",
            credential_scopes: ["github", "copilot"],
            collection: {
                version: 1,
                probe: { argv: ["gh", "auth", "status"] },
                sources: [{ type: "directory", path: "~/.config/gh", glob: "*", target: ".config/gh" }]
            },
            exposure: {
                version: 1,
                home_overlay: true,
                environment: { GH_CONFIG_DIR: "${PROFILE_HOME}/.config/gh" }
            },
            enabled: true
        }
    "#;

    let bundle = parse_settings_str(src).expect("parse settings");
    assert_eq!(bundle.version, 1);
    assert_eq!(bundle.settings.len(), 2);
    assert_eq!(bundle.execution_profiles.len(), 1);
    assert!(bundle.settings[0].schema.is_some());
    assert!(bundle.settings[1].expires_at.is_some());

    let rendered = settings_to_rexrap(&bundle);
    let reparsed = parse_settings_str(&rendered).expect("reparse rendered settings");
    assert_eq!(bundle.settings, reparsed.settings, "rendered:\n{rendered}");
    assert_eq!(
        bundle.execution_profiles, reparsed.execution_profiles,
        "rendered:\n{rendered}"
    );
}

#[test]
fn rejects_invalid_profile_objects_and_duplicate_declarations() {
    use crate::parse_settings_str;

    let invalid = parse_settings_str(
        r#"profile "bad" = {
            credential_scopes: ["github"],
            collection: { version: 2, sources: [{ type: "file", path: "~/.gitconfig", target: ".gitconfig" }] },
            exposure: { version: 1 }
        }"#,
    )
    .expect_err("unsupported profile version must fail");
    assert!(invalid.to_string().contains("version"));

    let duplicate = parse_settings_str(
        r#"
        profile "GitHub" = {
            credential_scopes: ["github"],
            collection: { version: 1, sources: [{ type: "file", path: "~/.gitconfig", target: ".gitconfig" }] },
            exposure: { version: 1 }
        }
        profile "github" = {
            credential_scopes: ["github"],
            collection: { version: 1, sources: [{ type: "file", path: "~/.gitconfig", target: ".gitconfig" }] },
            exposure: { version: 1 }
        }
        "#,
    )
    .expect_err("case-insensitive duplicate must fail");
    assert!(
        duplicate
            .to_string()
            .contains("duplicate execution profile")
    );
}

// header triggers ------------------------------------------------------------
