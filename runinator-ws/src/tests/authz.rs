//! who can see and do what: workflow visibility and permission from direct and team grants, and the
//! capability set each principal kind resolves to.

use super::*;
use std::collections::BTreeMap;

use runinator_broker::UiEventPublisher;
use runinator_engine::services::WorkflowAuthoring;
use runinator_models::{
    orchestration::AdapterTransport,
    rbac::{Action, ScopeRef},
};
use runinator_store::roles::NewAdapterDefinition;

async fn register_workflow_ownership(db: &SqliteDb, workflow_id: Uuid, org_id: Option<Uuid>) {
    let now = chrono::Utc::now();
    let tenant = org_id
        .and_then(|id| ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, Some(id)))
        .unwrap_or(ScopeRef::PLATFORM);
    db.put_resource_ownership(runinator_models::rbac::ResourceOwnership {
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        tenant,
        owner: tenant,
        created_by: None,
        authz_version: 1,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn visible_workflow_ids_include_direct_and_team_grants() {
    let (db, path) = test_db().await;
    let direct = save_workflow(&db, &workflow(None, "direct")).await.unwrap();
    let team = save_workflow(&db, &workflow(None, "team")).await.unwrap();
    register_workflow_ownership(&db, direct.id.unwrap(), None).await;
    register_workflow_ownership(&db, team.id.unwrap(), None).await;
    let user = db.create_user("member".into(), None, None).await.unwrap();
    let user_id = user.id.expect("user id");
    let team_record = db
        .create_team("ops".into(), ScopeRef::PLATFORM)
        .await
        .unwrap();
    let team_id = team_record.id.expect("team id");
    db.add_team_member(team_id, user_id, runinator_models::rbac::TeamRole::Member)
        .await
        .unwrap();
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: direct.id.expect("workflow id"),
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::View,
        created_at: chrono::Utc::now(),
    })
    .await
    .unwrap();
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: team.id.expect("workflow id"),
        principal_type: PrincipalType::Team,
        principal_id: team_id,
        permission: Permission::Run,
        created_at: chrono::Utc::now(),
    })
    .await
    .unwrap();

    let visible = AuthzChecker::new(
        &db,
        &AuthContext {
            principal_id: Some(user_id),
            session_id: None,
            platform_role: None,
            assignments: Vec::new(),
            system_role: None,
            action_ceiling: Vec::new(),
            kind: PrincipalKind::User,
            org_id: None,
        },
    )
    .visible_workflow_ids()
    .await
    .unwrap_or_else(|_| panic!("authorization lookup failed"))
    .expect("non-admin visibility set");

    assert_eq!(visible.len(), 2);
    assert!(visible.contains(&direct.id.unwrap()));
    assert!(visible.contains(&team.id.unwrap()));

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_listing_is_isolated_by_org() {
    use axum::extract::Query;

    let (db, path) = test_db().await;
    let db = Arc::new(db);

    // one workflow owned by org A, one platform-global (no org).
    let org_a = Uuid::now_v7();
    let org_b = Uuid::now_v7();
    let mut wf_a = workflow(None, "alpha");
    wf_a.org_id = Some(org_a);
    let wf_a = save_workflow(db.as_ref(), &wf_a).await.unwrap();
    let wf_a_id = wf_a.id.unwrap();
    let shared = save_workflow(db.as_ref(), &workflow(None, "shared"))
        .await
        .unwrap();
    let shared_id = shared.id.unwrap();
    register_workflow_ownership(db.as_ref(), wf_a_id, Some(org_a)).await;
    register_workflow_ownership(db.as_ref(), shared_id, None).await;

    // the org-B user is granted view on BOTH workflows, so only org scoping (not missing grants)
    // can hide org A's workflow from them.
    let user_id = db
        .create_user("orgb-user".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();
    db.create_grant(grant(
        wf_a_id,
        PrincipalType::User,
        user_id,
        Permission::View,
    ))
    .await
    .unwrap();
    db.create_grant(grant(
        shared_id,
        PrincipalType::User,
        user_id,
        Permission::View,
    ))
    .await
    .unwrap();

    // a member of org B sees the shared workflow but not org A's.
    let authoring = Arc::new(WorkflowAuthoring::new(
        db.clone(),
        UiEventPublisher::new(Arc::new(InMemoryBroker::new())),
    ));
    let ctx_b = AuthContext {
        principal_id: Some(user_id),
        session_id: None,
        platform_role: None,
        assignments: vec![runinator_models::rbac::RoleAssignment {
            principal_kind: PrincipalKind::User,
            principal_id: user_id,
            scope: ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, Some(org_b))
                .unwrap(),
            role: runinator_models::rbac::Role::Organization(OrgRole::Member),
            created_by: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: Some(org_b),
    };
    let (status, body) = crate::handlers::workflows::get_workflows::<SqliteDb>(
        Extension(db.clone()),
        Extension(authoring.clone()),
        Extension(ctx_b.clone()),
        Query(crate::handlers::workflows::WorkflowQuery { name: None }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names = workflow_list_names(&body);
    assert!(names.contains(&"shared".to_string()));
    assert!(!names.contains(&"alpha".to_string()));

    // and fetching org A's workflow directly is a not-found for the org-B caller.
    let (status, _) = crate::handlers::workflows::get_workflow::<SqliteDb>(
        Extension(db.clone()),
        Extension(authoring.clone()),
        Extension(ctx_b),
        Path(wf_a_id),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // the platform admin sees every workflow regardless of org.
    let admin_ctx = AuthContext {
        principal_id: None,
        session_id: None,
        platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::Service,
        org_id: None,
    };
    let (_, body) = crate::handlers::workflows::get_workflows::<SqliteDb>(
        Extension(db.clone()),
        Extension(authoring),
        Extension(admin_ctx),
        Query(crate::handlers::workflows::WorkflowQuery { name: None }),
    )
    .await;
    let names = workflow_list_names(&body);
    assert!(names.contains(&"alpha".to_string()));
    assert!(names.contains(&"shared".to_string()));
    let _ = shared_id;

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn platform_admin_can_view_orchestration_adapters_without_an_org() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let first_org = Uuid::now_v7();
    let second_org = Uuid::now_v7();
    let first_id = Uuid::now_v7();

    for (id, org_id, name) in [
        (first_id, first_org, "first"),
        (Uuid::now_v7(), second_org, "second"),
    ] {
        db.create_orchestration_adapter(
            NewAdapterDefinition {
                id,
                org_id,
                name: name.into(),
                kind: "generic_webhook".into(),
                kind_version: "1".into(),
                transport: AdapterTransport::Webhook,
                endpoint_identity: Uuid::now_v7().to_string(),
                configuration: json!({}),
                secret_bindings: BTreeMap::new(),
                identity_configuration: Value::Null,
                actor_id: None,
            },
            chrono::Utc::now(),
        )
        .await
        .unwrap();
    }

    let admin = AuthContext::disabled_platform_admin();
    let (status, Json(body)) = crate::handlers::adapters::list::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin.clone()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::OrchestrationAdapterList(adapters) = body else {
        panic!("expected an orchestration adapter list");
    };
    assert_eq!(adapters.len(), 2);
    assert!(adapters.iter().any(|adapter| adapter.org_id == first_org));
    assert!(adapters.iter().any(|adapter| adapter.org_id == second_org));

    let (status, _) = crate::handlers::adapters::get_one::<SqliteDb>(
        Extension(db.clone()),
        Extension(admin),
        Path(first_id),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    drop(db);
    let _ = std::fs::remove_file(path);
}

// Pull workflow names out of a WorkflowList API response for assertions.
fn workflow_list_names(body: &Json<crate::models::ApiResponse>) -> Vec<String> {
    match &body.0 {
        crate::models::ApiResponse::WorkflowList(list) => {
            list.iter().map(|w| w.name.clone()).collect()
        }
        _ => Vec::new(),
    }
}

#[tokio::test]
async fn workflow_permission_takes_highest_of_user_and_team_grants() {
    let (db, path) = test_db().await;
    let wf = save_workflow(&db, &workflow(None, "shared")).await.unwrap();
    let workflow_id = wf.id.expect("workflow id");
    register_workflow_ownership(&db, workflow_id, None).await;
    let user_id = db
        .create_user("member".into(), None, None)
        .await
        .unwrap()
        .id
        .expect("user id");
    let team_id = db
        .create_team("ops".into(), ScopeRef::PLATFORM)
        .await
        .unwrap()
        .id
        .expect("team id");
    db.add_team_member(team_id, user_id, runinator_models::rbac::TeamRole::Member)
        .await
        .unwrap();
    // a weak direct grant and a stronger team grant on the same workflow.
    db.create_grant(grant(
        workflow_id,
        PrincipalType::User,
        user_id,
        Permission::View,
    ))
    .await
    .unwrap();
    db.create_grant(grant(
        workflow_id,
        PrincipalType::Team,
        team_id,
        Permission::Edit,
    ))
    .await
    .unwrap();

    let ctx = user_ctx(user_id);
    let effective = AuthzChecker::new(&db, &ctx)
        .workflow_permission(workflow_id)
        .await;
    assert_eq!(effective, Some(Permission::Edit));

    // edit (and everything below it) is allowed; own is not.
    assert!(
        AuthzChecker::new(&db, &ctx)
            .require_workflow(workflow_id, Permission::Run)
            .await
            .is_ok()
    );
    assert!(
        AuthzChecker::new(&db, &ctx)
            .require_workflow(workflow_id, Permission::Edit)
            .await
            .is_ok()
    );
    let denied = AuthzChecker::new(&db, &ctx)
        .require_workflow(workflow_id, Permission::Own)
        .await;
    assert_eq!(
        denied.expect_err("own should be denied").0,
        StatusCode::FORBIDDEN
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_permission_is_none_without_a_grant() {
    let (db, path) = test_db().await;
    let wf = save_workflow(&db, &workflow(None, "private"))
        .await
        .unwrap();
    let workflow_id = wf.id.expect("workflow id");
    let user_id = db
        .create_user("stranger".into(), None, None)
        .await
        .unwrap()
        .id
        .expect("user id");

    let ctx = user_ctx(user_id);
    assert_eq!(
        AuthzChecker::new(&db, &ctx)
            .workflow_permission(workflow_id)
            .await,
        None
    );
    let denied = AuthzChecker::new(&db, &ctx)
        .require_workflow(workflow_id, Permission::View)
        .await;
    assert_eq!(
        denied.expect_err("view should be denied").0,
        StatusCode::NOT_FOUND
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn workflow_permission_admin_owns_everything_without_grants() {
    let (db, path) = test_db().await;
    let wf = save_workflow(&db, &workflow(None, "any")).await.unwrap();
    let workflow_id = wf.id.expect("workflow id");

    let admin = AuthContext {
        principal_id: None,
        session_id: None,
        platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: None,
    };
    assert_eq!(
        AuthzChecker::new(&db, &admin)
            .workflow_permission(workflow_id)
            .await,
        Some(Permission::Own)
    );
    assert!(
        AuthzChecker::new(&db, &admin)
            .require_workflow(workflow_id, Permission::Own)
            .await
            .is_ok()
    );

    let _ = std::fs::remove_file(path);
}

fn auth_ctx(is_platform_admin: bool, org_role: Option<OrgRole>) -> AuthContext {
    let principal_id = Uuid::now_v7();
    let org_id = org_role.map(|_| Uuid::now_v7());
    let now = chrono::Utc::now();
    AuthContext {
        principal_id: Some(principal_id),
        session_id: None,
        platform_role: is_platform_admin.then_some(runinator_models::rbac::PlatformRole::Admin),
        assignments: org_role
            .into_iter()
            .map(|role| runinator_models::rbac::RoleAssignment {
                principal_kind: PrincipalKind::User,
                principal_id,
                scope: ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, org_id)
                    .unwrap(),
                role: runinator_models::rbac::Role::Organization(role),
                created_by: None,
                created_at: now,
                updated_at: now,
            })
            .collect(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id,
    }
}

#[test]
fn scoped_view_matches_ui_event_egress_policy() {
    let org_a = Uuid::now_v7();
    let org_b = Uuid::now_v7();
    let admin = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        session_id: None,
        platform_role: Some(runinator_models::rbac::PlatformRole::Admin),
        assignments: Vec::new(),
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: Some(org_a),
    };
    let member_a = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        session_id: None,
        platform_role: None,
        assignments: vec![runinator_models::rbac::RoleAssignment {
            principal_kind: PrincipalKind::User,
            principal_id: Uuid::now_v7(),
            scope: ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, Some(org_a))
                .unwrap(),
            role: runinator_models::rbac::Role::Organization(OrgRole::Member),
            created_by: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: Some(org_a),
    };
    let member_b = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        session_id: None,
        platform_role: None,
        assignments: vec![runinator_models::rbac::RoleAssignment {
            principal_kind: PrincipalKind::User,
            principal_id: Uuid::now_v7(),
            scope: ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, Some(org_b))
                .unwrap(),
            role: runinator_models::rbac::Role::Organization(OrgRole::Member),
            created_by: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }],
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: Some(org_b),
    };

    let scope =
        |org| ScopeRef::new(runinator_models::rbac::ScopeKind::Organization, Some(org)).unwrap();
    // platform admin sees every scoped and unscoped event.
    assert!(admin.authorize_scope(Action::View, scope(org_a)));
    assert!(admin.authorize_scope(Action::View, scope(org_b)));
    assert!(admin.authorize_scope(Action::View, ScopeRef::PLATFORM));
    // org-only principals do not receive unscoped platform events.
    assert!(!member_a.authorize_scope(Action::View, ScopeRef::PLATFORM));
    assert!(!member_b.authorize_scope(Action::View, ScopeRef::PLATFORM));
    // scoped tips only reach the matching active org.
    assert!(member_a.authorize_scope(Action::View, scope(org_a)));
    assert!(!member_a.authorize_scope(Action::View, scope(org_b)));
    assert!(!member_b.authorize_scope(Action::View, scope(org_a)));
}

#[test]
fn platform_admin_holds_every_action() {
    let ctx = auth_ctx(true, None);
    for action in runinator_models::rbac::Action::ALL {
        assert!(
            ctx.authorize_scope(*action, ScopeRef::PLATFORM),
            "admin must hold {action:?}"
        );
    }
}

#[test]
fn disabled_admin_holds_every_action() {
    let ctx = AuthContext::disabled_platform_admin();
    assert!(
        runinator_models::rbac::Action::ALL
            .iter()
            .all(|action| ctx.authorize_scope(*action, ScopeRef::PLATFORM))
    );
}

#[test]
fn org_admin_holds_only_org_actions() {
    let ctx = auth_ctx(false, Some(OrgRole::Admin));
    let scope = ctx.selected_scope();
    assert!(ctx.authorize_scope(Action::MembersManage, scope));
    assert!(ctx.authorize_scope(Action::NodesOperate, scope));
    assert!(!ctx.authorize_scope(Action::Own, scope));
    assert!(!ctx.authorize_scope(Action::MembersManage, ScopeRef::PLATFORM));
}

#[test]
fn org_member_is_read_only() {
    let ctx = auth_ctx(false, Some(OrgRole::Member));
    let scope = ctx.selected_scope();
    assert!(ctx.authorize_scope(Action::View, scope));
    assert!(!ctx.authorize_scope(Action::Edit, scope));
}

#[test]
fn scoped_actions_gate_by_role() {
    let admin = auth_ctx(true, None);
    let member = auth_ctx(false, Some(OrgRole::Member));
    let org_admin = auth_ctx(false, Some(OrgRole::Admin));

    assert!(
        admin
            .require_scope_action(Action::MembersManage, ScopeRef::PLATFORM)
            .is_ok()
    );
    assert!(
        member
            .require_scope_action(Action::MembersManage, ScopeRef::PLATFORM)
            .is_err()
    );
    assert!(
        org_admin
            .require_scope_action(Action::MembersManage, ScopeRef::PLATFORM)
            .is_err()
    );
    assert!(
        org_admin
            .require_scope_action(Action::MembersManage, org_admin.selected_scope())
            .is_ok()
    );
}

#[tokio::test]
async fn me_returns_resolved_actions() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let user = db.create_user("member".into(), None, None).await.unwrap();
    let user_id = user.id.expect("user id");

    // an org admin sees the org capabilities but none of the platform ones.
    let org_id = Uuid::now_v7();
    let now = chrono::Utc::now();
    let ctx = AuthContext {
        principal_id: Some(user_id),
        session_id: None,
        platform_role: None,
        assignments: vec![runinator_models::rbac::RoleAssignment {
            principal_kind: PrincipalKind::User,
            principal_id: user_id,
            scope: ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(org_id),
            )
            .unwrap(),
            role: runinator_models::rbac::Role::Organization(OrgRole::Admin),
            created_by: None,
            created_at: now,
            updated_at: now,
        }],
        system_role: None,
        action_ceiling: Vec::new(),
        kind: PrincipalKind::User,
        org_id: Some(org_id),
    };
    let (status, Json(body)) =
        crate::handlers::auth::me::<SqliteDb>(Extension(db.clone()), Extension(ctx)).await;
    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::JsonValue(value) = body else {
        panic!("me response must be json");
    };
    let actions: Vec<String> = value
        .get("effective_actions")
        .and_then(|caps| caps.as_array())
        .expect("effective actions array")
        .iter()
        .filter_map(|cap| cap.as_str().map(str::to_string))
        .collect();
    assert!(actions.contains(&"members:manage".to_string()));
    assert!(!actions.contains(&"resource:own".to_string()));

    let _ = std::fs::remove_file(path);
}
