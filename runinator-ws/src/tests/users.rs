//! user administration and api keys: the last-enabled-admin guard, and key creation, rotation, and
//! ownership for admin and non-admin callers.

use super::*;
use std::{collections::BTreeMap, net::SocketAddr};

use axum::extract::ConnectInfo;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{Duration as ChronoDuration, Utc};
use runinator_auth::enroll::EnrollToken;
use runinator_models::auth::{
    AgentEnrollmentRequestBody, AgentEnrollmentToken, AgentEnrollmentTokenRecord,
    EnrollAgentRequest,
};
use runinator_models::replicas::{
    ReplicaHeartbeatRequest, ReplicaKind, ReplicaRegistrationRequest,
};
use runinator_utilities::secret_cipher::SecretCipher;

#[tokio::test]
async fn user_admin_handlers_preserve_last_enabled_admin() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let admin = db.create_user("admin".into(), None, None).await.unwrap();
    let admin_id = admin.id.expect("admin id");
    db.upsert_role_assignment(
        PrincipalKind::User,
        admin_id,
        runinator_models::rbac::ScopeRef::PLATFORM,
        runinator_models::rbac::Role::Platform(runinator_models::rbac::PlatformRole::Admin),
        None,
    )
    .await
    .unwrap();
    let ctx = AuthContext {
        principal_id: Some(admin_id),
        session_id: None,
        platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };

    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(ctx.clone()),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            platform_role: Some(runinator_models::rbac::PlatformRole::Member),
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
            platform_role: None,
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

    let second = db.create_user("second".into(), None, None).await.unwrap();
    db.upsert_role_assignment(
        PrincipalKind::User,
        second.id.unwrap(),
        runinator_models::rbac::ScopeRef::PLATFORM,
        runinator_models::rbac::Role::Platform(runinator_models::rbac::PlatformRole::Admin),
        None,
    )
    .await
    .unwrap();
    let (status, _) = crate::handlers::auth::update_user::<SqliteDb>(
        Extension(db.clone()),
        Extension(AuthContext {
            principal_id: second.id,
            session_id: None,
            platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
            assignments: Vec::new(),
            system_role: None,
            action_ceiling: Vec::new(),
            kind: PrincipalKind::User,
            org_id: None,
        }),
        Path(admin_id),
        Json(UpdateUserRequest {
            email: None,
            password: None,
            platform_role: Some(runinator_models::rbac::PlatformRole::Member),
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
    let admin = db.create_user("admin".into(), None, None).await.unwrap();
    let user = db.create_user("operator".into(), None, None).await.unwrap();
    let admin_ctx = AuthContext {
        principal_id: admin.id,
        session_id: None,
        platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };
    let user_id = user.id.expect("user id");

    let (status, _) = crate::handlers::auth::create_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin_ctx.clone()),
        Json(CreateApiKeyRequest {
            name: "operator key".into(),
            principal_kind: PrincipalKind::User,
            principal_id: user_id,
            system_role: None,
            org_id: None,
            action_ceiling: Vec::new(),
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let keys = db.list_api_keys(Some(user_id)).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "operator key");
    assert_eq!(keys[0].principal_kind, PrincipalKind::User);

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
    let caller = db.create_user("caller".into(), None, None).await.unwrap();
    let other = db.create_user("other".into(), None, None).await.unwrap();
    let caller_id = caller.id.expect("caller id");
    let other_id = other.id.expect("other id");

    let (status, _) = crate::handlers::auth::create_api_key::<SqliteDb>(
        Extension(db.clone()),
        Extension(AuthContext {
            principal_id: Some(caller_id),
            session_id: None,
            platform_role: None,
            assignments: Vec::new(),
            system_role: None,
            action_ceiling: Vec::new(),
            kind: PrincipalKind::User,
            org_id: None,
        }),
        Json(CreateApiKeyRequest {
            name: "attempted service key".into(),
            principal_kind: PrincipalKind::Service,
            principal_id: other_id,
            system_role: Some(runinator_models::rbac::SystemRole::Worker),
            org_id: None,
            action_ceiling: Vec::new(),
            expires_at: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let caller_keys = db.list_api_keys(Some(caller_id)).await.unwrap();
    assert!(caller_keys.is_empty());
    assert!(db.list_api_keys(Some(other_id)).await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}

async fn store_enrollment_token(
    db: &SqliteDb,
    labels: BTreeMap<String, String>,
    expires_at: chrono::DateTime<Utc>,
) -> EnrollToken {
    let token = EnrollToken::generate("https://runinator.example", None);
    let now = Utc::now();
    db.create_agent_enrollment_token(AgentEnrollmentTokenRecord {
        token: AgentEnrollmentToken {
            token_id: token.token_id.clone(),
            org_id: Some(Uuid::new_v4()),
            labels,
            service_url: token.service_url.clone(),
            spki_pin: None,
            expires_at,
            consumed_at: None,
            issued_by: None,
            created_at: now,
        },
        sealed_secret: SecretCipher::from_env().encrypt(&token.secret),
    })
    .await
    .unwrap();
    token
}

fn enrollment_request(token: &EnrollToken, labels: BTreeMap<String, String>) -> EnrollAgentRequest {
    let request_body = AgentEnrollmentRequestBody {
        instance_id: format!("agent-{}", token.token_id),
        display_name: Some("test agent".to_string()),
        labels,
    };
    let canonical = serde_json::to_vec(&request_body).unwrap();
    EnrollAgentRequest {
        token_id: token.token_id.clone(),
        request_body,
        proof: URL_SAFE_NO_PAD.encode(token.proof(&canonical)),
    }
}

async fn redeem(
    db: Arc<SqliteDb>,
    request: EnrollAgentRequest,
    ip: [u8; 4],
) -> (StatusCode, serde_json::Value) {
    let (status, Json(body)) = crate::handlers::auth::enroll_agent::<SqliteDb>(
        Extension(db),
        ConnectInfo(SocketAddr::from((ip, 49152))),
        Json(request),
    )
    .await;
    (status, serde_json::to_value(body).unwrap())
}

#[tokio::test]
async fn agent_enrollment_is_single_use_and_mints_a_scoped_non_admin_key() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let labels = BTreeMap::from([
        ("site".to_string(), "home".to_string()),
        ("gpu".to_string(), "true".to_string()),
    ]);
    let token = store_enrollment_token(
        db.as_ref(),
        labels.clone(),
        Utc::now() + ChronoDuration::minutes(5),
    )
    .await;
    let request = enrollment_request(
        &token,
        BTreeMap::from([("site".to_string(), "home".to_string())]),
    );

    let (status, _) = redeem(db.clone(), request.clone(), [127, 10, 0, 1]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = redeem(db.clone(), request, [127, 10, 0, 2]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let keys = db.list_api_keys(None).await.unwrap();
    assert_eq!(keys.len(), 1);
    let record = db
        .fetch_api_key(keys[0].id.expect("agent key id"))
        .await
        .unwrap()
        .expect("agent key record");
    assert_eq!(record.key.principal_kind, PrincipalKind::Service);
    assert_eq!(
        record.key.system_role,
        Some(runinator_models::rbac::SystemRole::Agent)
    );
    assert!(record.key.org_id.is_some());

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn agent_enrollment_rejections_are_uniform_and_labels_cannot_be_widened() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let allowed = BTreeMap::from([("site".to_string(), "home".to_string())]);

    let wrong_proof_token = store_enrollment_token(
        db.as_ref(),
        allowed.clone(),
        Utc::now() + ChronoDuration::minutes(5),
    )
    .await;
    let mut wrong_proof = enrollment_request(&wrong_proof_token, allowed.clone());
    wrong_proof.proof = URL_SAFE_NO_PAD.encode([0_u8; 32]);

    let expired_token = store_enrollment_token(
        db.as_ref(),
        allowed.clone(),
        Utc::now() - ChronoDuration::minutes(1),
    )
    .await;
    let expired = enrollment_request(&expired_token, allowed.clone());

    let widened_token = store_enrollment_token(
        db.as_ref(),
        allowed,
        Utc::now() + ChronoDuration::minutes(5),
    )
    .await;
    let widened = enrollment_request(
        &widened_token,
        BTreeMap::from([
            ("site".to_string(), "home".to_string()),
            ("gpu".to_string(), "true".to_string()),
        ]),
    );

    let missing = EnrollAgentRequest {
        token_id: "missing-token".to_string(),
        request_body: AgentEnrollmentRequestBody {
            instance_id: "missing".to_string(),
            display_name: None,
            labels: BTreeMap::new(),
        },
        proof: URL_SAFE_NO_PAD.encode([0_u8; 32]),
    };

    let failures = [
        redeem(db.clone(), wrong_proof, [127, 11, 0, 1]).await,
        redeem(db.clone(), expired, [127, 11, 0, 2]).await,
        redeem(db.clone(), widened, [127, 11, 0, 3]).await,
        redeem(db.clone(), missing, [127, 11, 0, 4]).await,
    ];
    for (status, body) in &failures {
        assert_eq!(*status, StatusCode::UNAUTHORIZED);
        assert_eq!(body, &failures[0].1);
    }
    assert!(db.list_api_keys(None).await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn agent_principals_cannot_mutate_another_agents_replica() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let owner_id = Uuid::new_v4();
    let owner = AuthContext {
        principal_id: Some(owner_id),
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: Some(runinator_models::rbac::SystemRole::Agent),
        action_ceiling: Vec::new(),
        kind: PrincipalKind::Service,
        org_id: None,
    };
    let request = ReplicaRegistrationRequest {
        replica_type: ReplicaKind::Worker,
        instance_id: "owned-agent".to_string(),
        runtime_id: "runtime-a".to_string(),
        display_name: None,
        host: None,
        port: None,
        base_path: None,
        version: None,
        attributes: json!({}),
    };
    let (status, _) = crate::handlers::replicas::register_replica::<SqliteDb>(
        Extension(db.clone()),
        Extension(owner),
        axum::http::HeaderMap::new(),
        ConnectInfo(SocketAddr::from(([127, 12, 0, 1], 49152))),
        Json(request),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let replica = db
        .fetch_replicas(None, None, Utc::now() - ChronoDuration::minutes(1))
        .await
        .unwrap()
        .remove(0);
    assert_eq!(replica.registered_by_principal_id, Some(owner_id));

    let intruder = AuthContext {
        principal_id: Some(Uuid::new_v4()),
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: Some(runinator_models::rbac::SystemRole::Agent),
        action_ceiling: Vec::new(),
        kind: PrincipalKind::Service,
        org_id: None,
    };
    let (status, _) = crate::handlers::replicas::heartbeat_replica::<SqliteDb>(
        Extension(db.clone()),
        Extension(intruder),
        axum::http::HeaderMap::new(),
        ConnectInfo(SocketAddr::from(([127, 12, 0, 2], 49152))),
        Path(replica.replica_id),
        Json(ReplicaHeartbeatRequest {
            runtime_id: "runtime-a".to_string(),
            display_name: Some("hijacked".to_string()),
            host: None,
            port: None,
            base_path: None,
            attributes: json!({}),
        }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_ne!(
        db.fetch_replica(replica.replica_id)
            .await
            .unwrap()
            .unwrap()
            .display_name
            .as_deref(),
        Some("hijacked")
    );

    let _ = std::fs::remove_file(path);
}
