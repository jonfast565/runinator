use super::*;
use runinator_comm::ConsumerProfile;
use uuid::Uuid;

fn auth() -> BrokerAuth {
    BrokerAuth::new(b"test-secret".to_vec(), None)
}

#[test]
fn rejects_garbage_and_accepts_issued_tokens() {
    let auth = auth();
    assert!(auth.verify("not-a-token").is_none());

    let config = AuthConfig {
        enabled: true,
        jwt_secret: b"test-secret".to_vec(),
        jwt_secret_previous: None,
        access_ttl_secs: 60,
        refresh_ttl_secs: 60,
    };
    let (token, _) =
        runinator_auth::issue_access_token(&config, Uuid::now_v7(), false, None, None).unwrap();
    assert!(auth.verify(&token).is_some());
}

#[test]
fn replica_token_carries_its_replica_claim() {
    let config = AuthConfig {
        enabled: true,
        jwt_secret: b"test-secret".to_vec(),
        jwt_secret_previous: None,
        access_ttl_secs: 60,
        refresh_ttl_secs: 60,
    };
    let replica = Uuid::now_v7();
    let (token, _) = runinator_auth::issue_replica_token(&config, Uuid::now_v7(), replica).unwrap();
    let claims = auth().verify(&token).expect("valid replica token");
    assert_eq!(claims.rid.as_deref(), Some(replica.to_string().as_str()));
}

#[test]
fn authorize_receive_enforces_replica_scope() {
    let replica = Uuid::now_v7();
    let other = Uuid::now_v7();
    let scoped = Claims {
        sub: Uuid::now_v7().to_string(),
        adm: false,
        iat: 0,
        exp: 0,
        jti: String::new(),
        rid: Some(replica.to_string()),
        org: None,
        orl: None,
    };

    // a scoped token may only receive for its own replica.
    let ok = ConsumerProfile::shared("d").with_replica_id(replica);
    assert!(super::super::server::authorize_receive(
        &AuthIdentity(Some(scoped.clone())),
        Some(&ok)
    )
    .is_ok());

    // presenting a different replica is forbidden.
    let bad = ConsumerProfile::shared("d").with_replica_id(other);
    assert!(super::super::server::authorize_receive(
        &AuthIdentity(Some(scoped.clone())),
        Some(&bad)
    )
    .is_err());

    // a scoped token with no profile (the untargeted path) is forbidden.
    assert!(super::super::server::authorize_receive(&AuthIdentity(Some(scoped)), None).is_err());

    // auth disabled: anything goes.
    assert!(super::super::server::authorize_receive(&AuthIdentity(None), None).is_ok());
}
