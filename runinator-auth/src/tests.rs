use super::*;

fn config() -> AuthConfig {
    AuthConfig {
        enabled: true,
        jwt_secret: b"test-secret-bytes".to_vec(),
        jwt_secret_previous: None,
        access_ttl_secs: 3600,
        refresh_ttl_secs: 86400,
    }
}

#[test]
fn password_hash_round_trips() {
    let hash = hash_password("hunter2").expect("hash");
    assert!(verify_password("hunter2", &hash));
    assert!(!verify_password("wrong", &hash));
}

#[test]
fn access_token_round_trips_and_carries_admin() {
    let cfg = config();
    let user_id = Uuid::new_v4();
    let (token, _exp) = issue_access_token(&cfg, user_id, true, None, None).expect("issue");
    let claims = verify_access_token(&cfg, &token).expect("verify");
    assert_eq!(claims.sub, user_id.to_string());
    assert!(claims.adm);
}

#[test]
fn access_token_rejected_under_wrong_secret() {
    let (token, _) =
        issue_access_token(&config(), Uuid::new_v4(), false, None, None).expect("issue");
    let other = AuthConfig {
        jwt_secret: b"different-secret".to_vec(),
        ..config()
    };
    assert!(verify_access_token(&other, &token).is_none());
}

#[test]
fn rotated_token_verifies_against_previous_secret() {
    // a token minted before rotation (signed with the old secret).
    let old = config();
    let (token, _) = issue_access_token(&old, Uuid::new_v4(), false, None, None).expect("issue");

    // after rotation the old secret moves to the previous slot; new tokens use a fresh primary.
    let rotated = AuthConfig {
        jwt_secret: b"new-primary-secret".to_vec(),
        jwt_secret_previous: Some(old.jwt_secret.clone()),
        ..config()
    };
    assert!(
        verify_access_token(&rotated, &token).is_some(),
        "pre-rotation token must stay valid during the overlap window"
    );

    // once the previous secret is dropped the old token is rejected.
    let retired = AuthConfig {
        jwt_secret_previous: None,
        ..rotated
    };
    assert!(verify_access_token(&retired, &token).is_none());
}

#[test]
fn api_key_hash_matches_only_the_issued_secret() {
    let key = new_api_key();
    assert_eq!(hash_secret(&key.secret), key.key_hash);
    assert_ne!(hash_secret("prefix.bogus"), key.key_hash);
    assert!(key.secret.starts_with(&key.prefix));
}
