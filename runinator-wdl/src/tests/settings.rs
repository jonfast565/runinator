//! the `.wdls` settings front end: secrets and config, and the values it refuses.

use super::*;

#[test]
fn parses_wdls_secrets_and_config() {
    use crate::parse_secrets_str;
    use runinator_models::settings::SettingKind;

    let src = r#"
        secret jira.token = "abc123"
        secret jira.api.key = "xyz"
        config jira.base_url = "https://acme.atlassian.net"
        config app.retries = 3
        config app.flags = { beta: true, region: "us" }
    "#;
    let bundle = parse_secrets_str(src).expect("parse wdls");
    assert_eq!(bundle.secrets.len(), 5);

    let token = &bundle.secrets[0];
    assert_eq!(token.scope, "jira");
    assert_eq!(token.name, "token");
    assert_eq!(token.kind, SettingKind::Secret);
    assert_eq!(token.value, Value::from("abc123"));

    // multi-segment names join with `/`, matching wdl secret addressing.
    assert_eq!(bundle.secrets[1].name, "api/key");

    let base = &bundle.secrets[2];
    assert_eq!(base.kind, SettingKind::Config);
    assert_eq!(base.value, Value::from("https://acme.atlassian.net"));

    assert_eq!(bundle.secrets[3].value, Value::from(3));

    let flags = &bundle.secrets[4];
    assert_eq!(flags.kind, SettingKind::Config);
    assert_eq!(flags.value.get("beta"), Some(&Value::from(true)));
    assert_eq!(flags.value.get("region"), Some(&Value::from("us")));
}
#[test]
fn rejects_wdls_reference_value() {
    use crate::parse_secrets_str;
    let err = parse_secrets_str("config app.url = config.other.url\n").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("literals"), "{message}");
}
#[test]
fn rejects_wdls_interpolated_value() {
    use crate::parse_secrets_str;
    let err = parse_secrets_str("secret app.k = \"a-${params.x}\"\n").unwrap_err();
    let message = format!("{err}");
    assert!(message.contains("interpolate"), "{message}");
}
#[test]
fn wdls_round_trips_through_export() {
    use crate::{parse_secrets_str, secrets_to_wdls};
    let src = r#"
        secret jira.token = "abc123"
        config jira.base_url = "https://acme.atlassian.net"
        config app.flags = { beta: true }
        config app.tags = ["x", "y"]
    "#;
    let bundle = parse_secrets_str(src).expect("parse");
    let rendered = secrets_to_wdls(&bundle);
    let reparsed = parse_secrets_str(&rendered).expect("reparse");
    assert_eq!(bundle.secrets, reparsed.secrets, "rendered:\n{rendered}");
}

// header triggers ------------------------------------------------------------
