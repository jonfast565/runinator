use runinator_models::settings::SettingKind;
use runinator_models::value::Value;

use super::{decode_config_schema, decode_config_value, validate_and_encode};

// the schema pinned in a config slot's stored bytes, mirroring how the handler reuses it on a
// value-only update.
fn pinned_schema(bytes: &[u8]) -> Option<Value> {
    decode_config_schema(bytes)
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
    assert_eq!(bytes, b"tok");
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
