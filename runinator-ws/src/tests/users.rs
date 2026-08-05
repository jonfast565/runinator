//! user administration and api keys: the last-enabled-admin guard, and key creation, rotation, and
//! ownership for admin and non-admin callers.

use super::*;

#[tokio::test]
async fn user_admin_handlers_preserve_last_enabled_admin() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let admin = db
        .create_user("admin".into(), None, true, None)
        .await
        .unwrap();
    let admin_id = admin.id.expect("admin id");
    let ctx = AuthContext {
        principal_id: Some(admin_id),
        is_admin: true,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
    };

    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx.clone()),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            is_admin: Some(false),
            disabled: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx.clone()),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            is_admin: None,
            disabled: Some(true),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = crate::handlers::auth::delete_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx),
        Path(admin_id),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let second = db
        .create_user("second".into(), None, true, None)
        .await
        .unwrap();
    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(AuthContext {
            principal_id: second.id,
            is_admin: true,
            kind: PrincipalKind::User,
            org_id: None,
            org_role: None,
        }),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            is_admin: Some(false),
            disabled: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn api_key_handlers_support_admin_user_keys_and_rotation() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let admin = db
        .create_user("admin".into(), None, true, None)
        .await
        .unwrap();
    let user = db
        .create_user("operator".into(), None, false, None)
        .await
        .unwrap();
    let admin_ctx = AuthContext {
        principal_id: admin.id,
        is_admin: true,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
    };
    let user_id = user.id.expect("user id");

    let (status, _) = crate::handlers::auth::create_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx.clone()),
        Json(CreateApiKeyRequest {
            name: "operator key".into(),
            user_id: Some(user_id),
            is_service: false,
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let keys = db.list_api_keys(Some(user_id)).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "operator key");
    assert!(!keys[0].is_service);

    let key_id = keys[0].id.expect("key id");
    let (status, _) = crate::handlers::auth::update_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx.clone()),
        Path(key_id),
        Json(UpdateApiKeyRequest {
            name: Some("renamed key".into()),
            expires_at: Some(None),
            disabled: Some(false),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let updated = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert_eq!(updated.key.name, "renamed key");

    let (status, _) = crate::handlers::auth::rotate_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx),
        Path(key_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let old = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert!(old.key.disabled);
    let keys = db.list_api_keys(Some(user_id)).await.unwrap();
    assert_eq!(keys.len(), 2);
    assert_eq!(keys.iter().filter(|key| !key.disabled).count(), 1);
    assert!(keys.iter().any(|key| key.name == "renamed key"));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn non_admin_api_key_creation_stays_owned_by_caller() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let caller = db
        .create_user("caller".into(), None, false, None)
        .await
        .unwrap();
    let other = db
        .create_user("other".into(), None, false, None)
        .await
        .unwrap();
    let caller_id = caller.id.expect("caller id");
    let other_id = other.id.expect("other id");

    let (status, _) = crate::handlers::auth::create_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(AuthContext {
            principal_id: Some(caller_id),
            is_admin: false,
            kind: PrincipalKind::User,
            org_id: None,
            org_role: None,
        }),
        Json(CreateApiKeyRequest {
            name: "attempted service key".into(),
            user_id: Some(other_id),
            is_service: true,
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let caller_keys = db.list_api_keys(Some(caller_id)).await.unwrap();
    assert_eq!(caller_keys.len(), 1);
    assert!(!caller_keys[0].is_service);
    assert_eq!(caller_keys[0].user_id, Some(caller_id));
    assert!(db.list_api_keys(Some(other_id)).await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}
