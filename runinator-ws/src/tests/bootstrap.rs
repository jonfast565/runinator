//! first-boot provisioning: the local admin credentials and service api key seeding creates (and
//! refuses to overwrite), plus the jwt secret the database is bootstrapped with.

use super::*;

#[tokio::test]
async fn seed_bootstrap_admin_creates_local_admin_credentials() {
    let (db, path) = test_db().await;

    seed_bootstrap_admin(&db, "admin:secret-pass", false)
        .await
        .unwrap();

    let user = db
        .fetch_user_by_username("admin".into())
        .await
        .unwrap()
        .expect("seeded user");
    let credential = db
        .fetch_local_credential("admin".into())
        .await
        .unwrap()
        .expect("seeded credential");

    assert!(
        db.list_principal_role_assignments(PrincipalKind::User, user.id.unwrap())
            .await
            .unwrap()
            .iter()
            .any(|a| a.role
                == runinator_models::rbac::Role::Platform(
                    runinator_models::rbac::PlatformRole::Admin
                ))
    );
    assert_eq!(db.count_users().await.unwrap(), 1);
    assert_eq!(credential.user.id, user.id);
    assert!(crate::auth::verify_password(
        "secret-pass",
        &credential.password_hash
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_admin_does_not_overwrite_existing_users() {
    let (db, path) = test_db().await;

    db.create_user("existing".into(), None, None).await.unwrap();

    seed_bootstrap_admin(&db, "admin:secret-pass", false)
        .await
        .unwrap();

    assert_eq!(db.count_users().await.unwrap(), 1);
    assert!(
        db.fetch_user_by_username("admin".into())
            .await
            .unwrap()
            .is_none()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_admin_force_resets_existing_admin() {
    let (db, path) = test_db().await;

    seed_bootstrap_admin(&db, "admin:old-pass", false)
        .await
        .unwrap();
    // a non-force re-seed must not touch the existing admin.
    seed_bootstrap_admin(&db, "admin:new-pass", false)
        .await
        .unwrap();
    let credential = db
        .fetch_local_credential("admin".into())
        .await
        .unwrap()
        .expect("credential");
    assert!(crate::auth::verify_password(
        "old-pass",
        &credential.password_hash
    ));

    // force reconciles the stale password without creating a duplicate user.
    seed_bootstrap_admin(&db, "admin:new-pass", true)
        .await
        .unwrap();
    let credential = db
        .fetch_local_credential("admin".into())
        .await
        .unwrap()
        .expect("credential");
    assert_eq!(db.count_users().await.unwrap(), 1);
    assert!(crate::auth::verify_password(
        "new-pass",
        &credential.password_hash
    ));
    assert!(!crate::auth::verify_password(
        "old-pass",
        &credential.password_hash
    ));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_admin_force_provisions_alongside_existing_users() {
    let (db, path) = test_db().await;

    db.create_user("existing".into(), None, None).await.unwrap();
    seed_bootstrap_admin(&db, "admin:secret-pass", true)
        .await
        .unwrap();

    let user = db
        .fetch_user_by_username("admin".into())
        .await
        .unwrap()
        .expect("seeded admin");
    assert!(
        db.list_principal_role_assignments(PrincipalKind::User, user.id.unwrap())
            .await
            .unwrap()
            .iter()
            .any(|a| a.role
                == runinator_models::rbac::Role::Platform(
                    runinator_models::rbac::PlatformRole::Admin
                ))
    );
    assert_eq!(db.count_users().await.unwrap(), 2);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_service_api_key_creates_admin_service_key() {
    let (db, path) = test_db().await;
    let raw_key = "localdev.runinator-local-dev-service-key";

    seed_bootstrap_service_api_key(&db, "local-dev", raw_key)
        .await
        .unwrap();

    let record = db
        .fetch_api_key_by_prefix("localdev".into())
        .await
        .unwrap()
        .expect("seeded api key");

    assert_eq!(record.key.name, "local-dev");
    assert_eq!(record.key.principal_kind, PrincipalKind::Service);
    assert!(
        db.list_principal_role_assignments(PrincipalKind::Service, record.key.principal_id)
            .await
            .unwrap()
            .iter()
            .any(|a| a.role
                == runinator_models::rbac::Role::Platform(
                    runinator_models::rbac::PlatformRole::Admin
                ))
    );
    assert_eq!(record.key_hash, crate::auth::hash_secret(raw_key));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn seed_bootstrap_service_api_key_is_idempotent_for_existing_prefix() {
    let (db, path) = test_db().await;
    let raw_key = "localdev.runinator-local-dev-service-key";

    seed_bootstrap_service_api_key(&db, "local-dev", raw_key)
        .await
        .unwrap();
    seed_bootstrap_service_api_key(&db, "local-dev", raw_key)
        .await
        .unwrap();

    assert_eq!(db.list_api_keys(None).await.unwrap().len(), 1);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn bootstrap_database_persists_explicit_jwt_secret() {
    let (db, path) = test_db().await;

    let db = Arc::new(db);
    bootstrap_database(
        &db,
        &BootstrapOptions {
            auth_jwt_secret: Some("explicit-secret".into()),
            auth_jwt_secret_previous: None,
            auth_bootstrap_admin: None,
            auth_bootstrap_admin_force: false,
            auth_bootstrap_service_api_key: None,
            auth_bootstrap_service_api_key_name: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(
        load_jwt_secret(db.as_ref()).await.unwrap(),
        b"explicit-secret".to_vec()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn bootstrap_database_generates_jwt_secret_once() {
    let (db, path) = test_db().await;

    let db = Arc::new(db);
    bootstrap_database(&db, &BootstrapOptions::default())
        .await
        .unwrap();
    let first = load_jwt_secret(db.as_ref()).await.unwrap();

    bootstrap_database(&db, &BootstrapOptions::default())
        .await
        .unwrap();
    let second = load_jwt_secret(db.as_ref()).await.unwrap();

    assert!(!first.is_empty());
    assert_eq!(first, second);

    let _ = std::fs::remove_file(path);
}
