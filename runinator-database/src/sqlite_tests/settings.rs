//! the settings store: config/secret rows keyed by kind+scope+name, and the JWT signing secret
//! that is encrypted at rest and migrated forward from a legacy plaintext row.

use super::*;

#[tokio::test]
async fn settings_round_trip_by_kind_scope_name() {
    let path = std::env::temp_dir().join(format!(
        "runinator-settings-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // insert a secret and a config that share a scope/name but differ by kind: they must not collide.
    db.upsert_setting(
        SettingKind::Secret,
        "jira".into(),
        "token".into(),
        b"cipher-a".to_vec(),
        100,
    )
    .await
    .unwrap();
    db.upsert_setting(
        SettingKind::Config,
        "jira".into(),
        "token".into(),
        b"cipher-b".to_vec(),
        200,
    )
    .await
    .unwrap();

    let secret = db
        .fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(secret.value, b"cipher-a");
    assert_eq!(secret.updated_at, 100);
    assert_eq!(secret.kind, SettingKind::Secret);
    assert!(!secret.id.is_nil());

    let config = db
        .fetch_setting(SettingKind::Config, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(config.value, b"cipher-b");

    // upsert replaces value and timestamp in place.
    db.upsert_setting(
        SettingKind::Secret,
        "jira".into(),
        "token".into(),
        b"cipher-c".to_vec(),
        300,
    )
    .await
    .unwrap();
    let updated = db
        .fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.value, b"cipher-c");
    assert_eq!(updated.updated_at, 300);
    assert_eq!(
        updated.id, secret.id,
        "value updates preserve the setting UUID"
    );

    // list returns both rows; delete is kind-scoped.
    assert_eq!(db.list_settings().await.unwrap().len(), 2);
    db.delete_setting(SettingKind::Secret, "jira".into(), "token".into())
        .await
        .unwrap();
    assert!(
        db.fetch_setting(SettingKind::Secret, "jira".into(), "token".into())
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        db.fetch_setting(SettingKind::Config, "jira".into(), "token".into())
            .await
            .unwrap()
            .is_some()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn jwt_secret_is_encrypted_at_rest_and_round_trips() {
    let path = std::env::temp_dir().join(format!(
        "runinator-jwt-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // generate and persist a fresh signing secret.
    let secret = crate::ensure_jwt_secret(&db, None).await.unwrap();
    assert_eq!(secret.len(), 48);

    // the stored bytes must be sealed (carry the aead header), never the raw secret.
    let stored = db
        .fetch_setting(SettingKind::Secret, "auth".into(), "jwt_secret".into())
        .await
        .unwrap()
        .unwrap()
        .value;
    assert!(
        runinator_secrets::secret_cipher::SecretCipher::is_sealed(&stored),
        "jwt secret must be encrypted at rest"
    );
    assert_ne!(
        stored, secret,
        "stored value must not equal the plaintext secret"
    );

    // loading transparently decrypts back to the same plaintext.
    let loaded = crate::load_jwt_secret(&db).await.unwrap();
    assert_eq!(loaded, secret);

    let _ = std::fs::remove_file(path);
}
