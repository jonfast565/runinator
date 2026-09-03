use super::*;
use runinator_models::execution_profiles::{
    ExecutionProfile, ExecutionProfileCollectionSpec, ExecutionProfileExposureSpec,
    ExecutionProfileHealth, ExecutionProfileRevision,
};

#[tokio::test]
async fn execution_profile_publications_are_atomic_and_org_scoped() {
    let path = std::env::temp_dir().join(format!(
        "runinator-execution-profiles-{}.db",
        Uuid::new_v4()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let org_id = Uuid::new_v4();
    let id = Uuid::new_v4();
    let now = Utc::now();
    let profile = ExecutionProfile {
        id,
        org_id: Some(org_id),
        name: "aws-production".into(),
        description: "fixture".into(),
        credential_scopes: vec!["aws".into()],
        collection: ExecutionProfileCollectionSpec::default(),
        exposure: ExecutionProfileExposureSpec::default(),
        config_version: 1,
        config_digest: "config".into(),
        enabled: true,
        current_revision: None,
        current_digest: None,
        current_publisher_id: None,
        published_at: None,
        expires_at: None,
        refresh_requested_at: None,
        health: ExecutionProfileHealth::Unpublished,
        last_error: None,
        created_at: now,
        updated_at: now,
    };
    db.upsert_execution_profile(&profile).await.unwrap();
    assert!(db.list_execution_profiles(None).await.unwrap().is_empty());
    assert_eq!(
        db.list_execution_profiles(Some(org_id))
            .await
            .unwrap()
            .len(),
        1
    );

    let publisher_id = Uuid::new_v4();
    db.insert_execution_profile_revision(&ExecutionProfileRevision {
        profile_id: id,
        revision: 1,
        digest: "bundle".into(),
        size_bytes: 42,
        publisher_id: Some(publisher_id),
        expires_at: Some(now + Duration::hours(1)),
        created_at: now,
        uri: "blob://execution-profiles/profile/1.bundle".into(),
    })
    .await
    .unwrap();

    let published = db.fetch_execution_profile(id).await.unwrap().unwrap();
    assert_eq!(published.current_revision, Some(1));
    assert_eq!(published.current_digest.as_deref(), Some("bundle"));
    assert_eq!(published.current_publisher_id, Some(publisher_id));
    assert_eq!(published.health, ExecutionProfileHealth::Ready);
    drop(db);
    let _ = std::fs::remove_file(path);
}
