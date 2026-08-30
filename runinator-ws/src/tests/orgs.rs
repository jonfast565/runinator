//! org membership handlers and the quota/usage accounting scale decisions read.

use super::*;

fn org_context(principal_id: Uuid, org_id: Uuid, role: OrgRole) -> AuthContext {
    let now = chrono::Utc::now();
    AuthContext {
        principal_id: Some(principal_id),
        session_id: None,
        platform_role: None,
        assignments: vec![runinator_models::rbac::RoleAssignment {
            principal_kind: PrincipalKind::User,
            principal_id,
            scope: runinator_models::rbac::ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(org_id),
            )
            .unwrap(),
            role: runinator_models::rbac::Role::Organization(role),
            created_by: None,
            created_at: now,
            updated_at: now,
        }],
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: Some(org_id),
    }
}

#[tokio::test]
async fn org_scale_enforces_node_and_budget_quotas() {
    use runinator_models::billing::{OrgQuota, ScaleOrgNodesRequest};
    use runinator_models::provisioning::ProvisionBackend;
    use runinator_models::replicas::ReplicaKind;
    use runinator_provisioner::ProvisionerRegistry;

    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let registry = Arc::new(ProvisionerRegistry::default());
    let org_id = db
        .create_org("Acme".into(), "acme".into())
        .await
        .unwrap()
        .id
        .unwrap();

    // cap workers at 2 and set a monthly budget of 20000¢.
    db.upsert_org_quota(OrgQuota {
        org_id,
        max_nodes_per_kind: [("worker".to_string(), 2u32)].into_iter().collect(),
        max_monthly_cents: 20_000,
    })
    .await
    .unwrap();

    let admin = org_context(Uuid::now_v7(), org_id, OrgRole::Admin);
    let scale = |desired: u32| ScaleOrgNodesRequest {
        backend: ProvisionBackend::Supervisor,
        kind: ReplicaKind::Worker,
        desired,
    };

    // exceeding the node cap is rejected.
    let (status, _) = crate::handlers::billing::scale_org_nodes::<SqliteDb>(
        Extension(db.clone()),
        Extension(registry.clone()),
        Extension(admin.clone()),
        Path(org_id),
        ValidatedJson(scale(5)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 2 workers = 2 * 25¢ * 730h = 36500¢ > 20000¢ budget, so it is rejected on cost too.
    let (status, _) = crate::handlers::billing::scale_org_nodes::<SqliteDb>(
        Extension(db.clone()),
        Extension(registry.clone()),
        Extension(admin.clone()),
        Path(org_id),
        ValidatedJson(scale(2)),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // 1 worker = 18250¢ fits under both caps and records the allocation.
    let (status, _) = crate::handlers::billing::scale_org_nodes::<SqliteDb>(
        Extension(db.clone()),
        Extension(registry.clone()),
        Extension(admin),
        Path(org_id),
        ValidatedJson(scale(1)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let groups = db.list_org_resource_groups(org_id).await.unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].desired, 1);

    let _ = std::fs::remove_file(path);
}

#[test]
fn usage_integration_accrues_node_hours_and_cost() {
    use chrono::{Duration as ChronoDuration, Utc};
    use runinator_models::billing::{RateCard, UsageSample};
    use runinator_models::provisioning::ProvisionBackend;
    use runinator_models::replicas::ReplicaKind;

    let org_id = Uuid::now_v7();
    let start = Utc::now();
    // 2 workers held for exactly one hour, then observed again (the trailing sample closes the window).
    let samples = vec![
        UsageSample {
            org_id,
            backend: ProvisionBackend::Supervisor,
            kind: ReplicaKind::Worker,
            node_count: 2,
            sampled_at: start,
        },
        UsageSample {
            org_id,
            backend: ProvisionBackend::Supervisor,
            kind: ReplicaKind::Worker,
            node_count: 2,
            sampled_at: start + ChronoDuration::hours(1),
        },
    ];
    let usage =
        crate::handlers::billing::integrate_usage(org_id, samples, &RateCard::default_card());
    // 2 nodes * 1 hour = 2 node-hours; 2 * 25¢/h = 50¢.
    assert_eq!(usage.node_hours.get("worker").copied(), Some(2.0));
    assert_eq!(usage.accrued_cents, 50);
}

#[tokio::test]
async fn org_handlers_enforce_membership_roles_and_last_owner() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);

    // a non-admin user self-serves an org and becomes its owner.
    let alice = db.create_user("alice".into(), None, None).await.unwrap();
    let alice_id = alice.id.unwrap();
    let alice_ctx = AuthContext {
        principal_id: Some(alice_id),
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };
    let (status, _) = crate::handlers::orgs::create_org::<SqliteDb>(
        Extension(db.clone()),
        Extension(alice_ctx.clone()),
        ValidatedJson(CreateOrgRequest {
            name: "Acme Corp".into(),
            slug: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let orgs = db.list_user_orgs(alice_id).await.unwrap();
    assert_eq!(orgs.len(), 1);
    assert_eq!(orgs[0].1, OrgRole::Owner);
    assert_eq!(orgs[0].0.slug, "acme-corp");
    let org_id = orgs[0].0.id.unwrap();

    // a switched-in owner context can rename the org.
    let owner_ctx = org_context(alice_id, org_id, OrgRole::Owner);
    let (status, _) = crate::handlers::orgs::update_org::<SqliteDb>(
        Extension(db.clone()),
        Extension(owner_ctx.clone()),
        Path(org_id),
        ValidatedJson(UpdateOrgRequest {
            name: Some("Acme Inc".into()),
            disabled: None,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // a non-member (no org context) is forbidden from reading the org.
    let bob = db.create_user("bob".into(), None, None).await.unwrap();
    let bob_id = bob.id.unwrap();
    let bob_ctx = AuthContext {
        principal_id: Some(bob_id),
        session_id: None,
        platform_role: None,
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };
    let (status, _) = crate::handlers::orgs::get_org::<SqliteDb>(
        Extension(db.clone()),
        Extension(bob_ctx),
        Path(org_id),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // the owner adds bob as a member.
    let (status, _) = crate::handlers::orgs::add_org_member::<SqliteDb>(
        Extension(db.clone()),
        Extension(owner_ctx.clone()),
        Path(org_id),
        ValidatedJson(AddOrgMemberRequest {
            user_id: bob_id,
            role: OrgRole::Member,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(db.list_org_members(org_id).await.unwrap().len(), 2);

    // demoting the sole owner is rejected so the org always keeps an owner.
    let (status, _) = crate::handlers::orgs::update_org_member::<SqliteDb>(
        Extension(db.clone()),
        Extension(owner_ctx),
        Path((org_id, alice_id)),
        ValidatedJson(UpdateOrgMemberRequest {
            role: OrgRole::Member,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let _ = std::fs::remove_file(path);
}
