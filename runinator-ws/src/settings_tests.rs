use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use runinator_models::settings::SettingKind;
use runinator_models::value::Value;
use runinator_utilities::credential_store::{CredentialStore, LocalEncryptedCredentialStore};

use super::{decode_config_value, validate_and_encode};

fn temp_store() -> (LocalEncryptedCredentialStore, std::path::PathBuf) {
    // a process-global counter keeps paths unique even when parallel tests
    // sample the same timestamp, so one test's cleanup cannot wipe another's store.
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("runinator-ws-settings-{unique}-{seq}.json"));
    (LocalEncryptedCredentialStore::new(&path, "key"), path)
}

#[test]
fn secret_must_be_a_non_empty_string() {
    let (store, path) = temp_store();
    assert!(
        validate_and_encode(&store, SettingKind::Secret, "s", "n", &Value::from(7), None).is_err()
    );
    assert!(
        validate_and_encode(
            &store,
            SettingKind::Secret,
            "s",
            "n",
            &Value::String(String::new()),
            None
        )
        .is_err()
    );
    let bytes = validate_and_encode(
        &store,
        SettingKind::Secret,
        "s",
        "n",
        &Value::String("tok".into()),
        None,
    )
    .unwrap();
    assert_eq!(bytes, b"tok");
    let _ = std::fs::remove_file(path);
}

#[test]
fn config_infers_schema_from_value_when_undeclared() {
    let (store, path) = temp_store();

    // first write with no schema infers one from the value and persists it.
    let bytes = validate_and_encode(
        &store,
        SettingKind::Config,
        "api",
        "url",
        &Value::String("https://x".into()),
        None,
    )
    .unwrap();
    store
        .put(SettingKind::Config, "api", "url", &bytes)
        .unwrap();

    // a value of the same inferred type is accepted.
    assert!(
        validate_and_encode(
            &store,
            SettingKind::Config,
            "api",
            "url",
            &Value::String("https://y".into()),
            None,
        )
        .is_ok()
    );

    // a value that contradicts the inferred type is rejected.
    let err = validate_and_encode(
        &store,
        SettingKind::Config,
        "api",
        "url",
        &Value::from(7),
        None,
    )
    .unwrap_err();
    assert!(err.contains("does not match schema"), "{err}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn config_object_shape_can_evolve_but_known_fields_type_check() {
    let (store, path) = temp_store();

    // first write infers an open struct from the object's fields.
    let bytes = validate_and_encode(
        &store,
        SettingKind::Config,
        "svc",
        "options",
        &runinator_models::json!({ "url": "https://x", "retries": 3 }),
        None,
    )
    .unwrap();
    store
        .put(SettingKind::Config, "svc", "options", &bytes)
        .unwrap();

    // adding and dropping fields is allowed (shape can evolve).
    assert!(
        validate_and_encode(
            &store,
            SettingKind::Config,
            "svc",
            "options",
            &runinator_models::json!({ "url": "https://y", "timeout": 30 }),
            None,
        )
        .is_ok()
    );

    // a known field with the wrong type is still rejected.
    let err = validate_and_encode(
        &store,
        SettingKind::Config,
        "svc",
        "options",
        &runinator_models::json!({ "url": "https://y", "retries": "lots" }),
        None,
    )
    .unwrap_err();
    assert!(err.contains("does not match schema"), "{err}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn config_validates_value_against_schema() {
    let (store, path) = temp_store();
    let schema = runinator_models::json!({ "type": "string" });

    // a mismatching value is rejected.
    assert!(
        validate_and_encode(
            &store,
            SettingKind::Config,
            "api",
            "url",
            &Value::from(7),
            Some(&schema),
        )
        .is_err()
    );

    // a matching value encodes, and round-trips back through decode.
    let value = Value::String("https://x".into());
    let bytes = validate_and_encode(
        &store,
        SettingKind::Config,
        "api",
        "url",
        &value,
        Some(&schema),
    )
    .unwrap();
    assert_eq!(decode_config_value(&bytes), value);
    let _ = std::fs::remove_file(path);
}

#[test]
fn config_reuses_stored_schema_on_value_only_update() {
    let (store, path) = temp_store();
    let schema = runinator_models::json!({ "type": "integer" });

    // first write declares and persists the schema.
    let bytes = validate_and_encode(
        &store,
        SettingKind::Config,
        "tuning",
        "retries",
        &Value::from(3),
        Some(&schema),
    )
    .unwrap();
    store
        .put(SettingKind::Config, "tuning", "retries", &bytes)
        .unwrap();

    // a later value-only update reuses the stored schema and still type-checks.
    assert!(
        validate_and_encode(
            &store,
            SettingKind::Config,
            "tuning",
            "retries",
            &Value::from(5),
            None,
        )
        .is_ok()
    );
    assert!(
        validate_and_encode(
            &store,
            SettingKind::Config,
            "tuning",
            "retries",
            &Value::String("five".into()),
            None,
        )
        .is_err()
    );
    let _ = std::fs::remove_file(path);
}
