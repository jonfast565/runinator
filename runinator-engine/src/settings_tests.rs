use runinator_models::settings::SettingKind;
use runinator_models::value::Value;
use runinator_store::DatabaseImpl;

use super::{
    decode_config_schema, decode_config_value, decode_secret, load_persisted_server_settings,
    load_server_settings, save_server_settings, validate_and_encode,
    validate_and_encode_with_expiry,
};

// the schema pinned in a config slot's stored bytes, mirroring how the handler reuses it on a
// value-only update.
fn pinned_schema(bytes: &[u8]) -> Option<Value> {
    decode_config_schema(bytes)
}

#[tokio::test]
async fn server_settings_round_trip_as_one_validated_policy() {
    let path = std::env::temp_dir().join(format!(
        "runinator-server-settings-{}.db",
        uuid::Uuid::new_v4()
    ));
    let db = runinator_database::sqlite::SqliteDb::new(path.to_str().unwrap())
        .await
        .unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let mut settings = runinator_models::server_settings::ServerSettings::default();
    settings.orchestration.workflow_vm_poll_interval_ms = 500;
    settings.notifications.delivery_timeout_seconds = 45;
    settings.archiver.interval_seconds = 120;
    settings.archiver.dry_run = true;
    assert!(load_persisted_server_settings(&db).await.unwrap().is_none());
    save_server_settings(&db, &settings).await.unwrap();

    assert_eq!(load_server_settings(&db).await.unwrap(), settings);
    assert_eq!(
        load_persisted_server_settings(&db).await.unwrap(),
        Some(settings)
    );
}

#[test]
fn secret_must_be_a_non_empty_string() {
    assert!(
        validate_and_encode(SettingKind::Secret, "s", "n", &Value::from(7), None, None).is_err()
    );
    assert!(
        validate_and_encode(
            SettingKind::Secret,
            "s",
            "n",
            &Value::String(String::new()),
            None,
            None,
        )
        .is_err()
    );
    // a whitespace-only value does not satisfy a required secret.
    assert!(
        validate_and_encode(
            SettingKind::Secret,
            "s",
            "n",
            &Value::String("   ".into()),
            None,
            None,
        )
        .is_err()
    );
    let bytes = validate_and_encode(
        SettingKind::Secret,
        "s",
        "n",
        &Value::String("tok".into()),
        None,
        None,
    )
    .unwrap();
    assert!(bytes.starts_with(b"runinator-secret:v1:"));
    assert_eq!(decode_secret(&bytes).unwrap().value, "tok");
}

#[test]
fn secret_expiry_is_stored_with_the_value() {
    let expires_at = "2026-09-01T12:00:00Z".parse().unwrap();
    let bytes = validate_and_encode_with_expiry(
        SettingKind::Secret,
        "s",
        "n",
        &Value::String("tok".into()),
        None,
        None,
        Some(expires_at),
    )
    .unwrap();
    let stored = decode_secret(&bytes).unwrap();
    assert_eq!(stored.value, "tok");
    assert_eq!(stored.expires_at, Some(expires_at));
}

#[test]
fn config_rejects_secret_expiry_metadata() {
    let expires_at = "2026-09-01T12:00:00Z".parse().unwrap();
    let error = validate_and_encode_with_expiry(
        SettingKind::Config,
        "api",
        "url",
        &Value::String("https://example.com".into()),
        None,
        None,
        Some(expires_at),
    )
    .unwrap_err();
    assert!(error.contains("cannot carry secret expiry metadata"));
}

#[test]
fn config_infers_schema_from_value_when_undeclared() {
    // first write with no schema infers one from the value and persists it.
    let bytes = validate_and_encode(
        SettingKind::Config,
        "api",
        "url",
        &Value::String("https://x".into()),
        None,
        None,
    )
    .unwrap();
    let stored = pinned_schema(&bytes);

    // a value of the same inferred type is accepted against the pinned schema.
    assert!(
        validate_and_encode(
            SettingKind::Config,
            "api",
            "url",
            &Value::String("https://y".into()),
            None,
            stored.as_ref(),
        )
        .is_ok()
    );

    // a value that contradicts the inferred type is rejected.
    let err = validate_and_encode(
        SettingKind::Config,
        "api",
        "url",
        &Value::from(7),
        None,
        stored.as_ref(),
    )
    .unwrap_err();
    assert!(err.contains("does not match schema"), "{err}");
}

#[test]
fn config_object_shape_can_evolve_but_known_fields_type_check() {
    // first write infers an open struct from the object's fields.
    let bytes = validate_and_encode(
        SettingKind::Config,
        "svc",
        "options",
        &runinator_models::json!({ "url": "https://x", "retries": 3 }),
        None,
        None,
    )
    .unwrap();
    let stored = pinned_schema(&bytes);

    // adding and dropping fields is allowed (shape can evolve).
    assert!(
        validate_and_encode(
            SettingKind::Config,
            "svc",
            "options",
            &runinator_models::json!({ "url": "https://y", "timeout": 30 }),
            None,
            stored.as_ref(),
        )
        .is_ok()
    );

    // a known field with the wrong type is still rejected.
    let err = validate_and_encode(
        SettingKind::Config,
        "svc",
        "options",
        &runinator_models::json!({ "url": "https://y", "retries": "lots" }),
        None,
        stored.as_ref(),
    )
    .unwrap_err();
    assert!(err.contains("does not match schema"), "{err}");
}

#[test]
fn config_validates_value_against_schema() {
    let schema = runinator_models::json!({ "type": "string" });

    // a mismatching value is rejected.
    assert!(
        validate_and_encode(
            SettingKind::Config,
            "api",
            "url",
            &Value::from(7),
            Some(&schema),
            None,
        )
        .is_err()
    );

    // a matching value encodes, and round-trips back through decode.
    let value = Value::String("https://x".into());
    let bytes = validate_and_encode(
        SettingKind::Config,
        "api",
        "url",
        &value,
        Some(&schema),
        None,
    )
    .unwrap();
    assert_eq!(decode_config_value(&bytes), value);
}

#[test]
fn config_reuses_stored_schema_on_value_only_update() {
    let schema = runinator_models::json!({ "type": "integer" });

    // first write declares and persists the schema.
    let bytes = validate_and_encode(
        SettingKind::Config,
        "tuning",
        "retries",
        &Value::from(3),
        Some(&schema),
        None,
    )
    .unwrap();
    let stored = pinned_schema(&bytes);

    // a later value-only update reuses the stored schema and still type-checks.
    assert!(
        validate_and_encode(
            SettingKind::Config,
            "tuning",
            "retries",
            &Value::from(5),
            None,
            stored.as_ref(),
        )
        .is_ok()
    );
    assert!(
        validate_and_encode(
            SettingKind::Config,
            "tuning",
            "retries",
            &Value::String("five".into()),
            None,
            stored.as_ref(),
        )
        .is_err()
    );
}
