//! who can see and do what: workflow visibility and permission from direct and team grants, and the
//! capability set each principal kind resolves to.

use super::*;

#[tokio::test]
async fn visible_workflow_ids_include_direct_and_team_grants() {
    let (db, path) = test_db().await;
    let direct = save_workflow(&db, &workflow(None, "direct")).await.unwrap();
    let team = save_workflow(&db, &workflow(None, "team")).await.unwrap();
    let user = db
        .create_user("member".into(), None, false, None)
        .await
        .unwrap();
    let user_id = user.id.expect("user id");
    let team_record = db.create_team("ops".into()).await.unwrap();
    let team_id = team_record.id.expect("team id");
    db.add_team_member(team_id, user_id).await.unwrap();
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
            is_admin: false,
            kind: PrincipalKind::User,
            org_id: None,
            org_role: None,
        },
    )
    .visible_workflow_ids()
    .await
    .expect("scoped set");

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

    // the org-B user is granted view on BOTH workflows, so only org scoping (not missing grants)
    // can hide org A's workflow from them.
    let user_id = db
        .create_user("orgb-user".into(), None, false, None)
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
    let ctx_b = AuthContext {
        principal_id: Some(user_id),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: Some(org_b),
        org_role: Some(OrgRole::Member),
    };
    let (status, body) = crate::handlers::workflows::get_workflows::<SqliteDb>(
        Extension(db.clone()),
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
        Extension(ctx_b),
        Path(wf_a_id),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // the platform admin sees every workflow regardless of org.
    let admin_ctx = AuthContext {
        principal_id: None,
        is_admin: true,
        kind: PrincipalKind::Service,
        org_id: None,
        org_role: None,
    };
    let (_, body) = crate::handlers::workflows::get_workflows::<SqliteDb>(
        Extension(db.clone()),
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

// pull workflow names out of a WorkflowList api response for assertions.
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
    let user_id = db
        .create_user("member".into(), None, false, None)
        .await
        .unwrap()
        .id
        .expect("user id");
    let team_id = db
        .create_team("ops".into())
        .await
        .unwrap()
        .id
        .expect("team id");
    db.add_team_member(team_id, user_id).await.unwrap();
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
        .create_user("stranger".into(), None, false, None)
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
        StatusCode::FORBIDDEN
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
        is_admin: true,
        kind: PrincipalKind::User,
        org_id: None,
        org_role: None,
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

fn auth_ctx(is_admin: bool, org_role: Option<OrgRole>) -> AuthContext {
    AuthContext {
        principal_id: Some(Uuid::now_v7()),
        is_admin,
        kind: PrincipalKind::User,
        org_id: org_role.map(|_| Uuid::now_v7()),
        org_role,
    }
}

#[test]
fn org_visible_matches_ui_event_egress_policy() {
    let org_a = Uuid::now_v7();
    let org_b = Uuid::now_v7();
    let admin = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        is_admin: true,
        kind: PrincipalKind::User,
        org_id: Some(org_a),
        org_role: Some(OrgRole::Admin),
    };
    let member_a = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: Some(org_a),
        org_role: Some(OrgRole::Member),
    };
    let member_b = AuthContext {
        principal_id: Some(Uuid::now_v7()),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: Some(org_b),
        org_role: Some(OrgRole::Member),
    };

    // platform admin sees every scoped and unscoped event.
    assert!(admin.org_visible(Some(org_a)));
    assert!(admin.org_visible(Some(org_b)));
    assert!(admin.org_visible(None));
    // unscoped (rollout / global) tips stay visible to every client.
    assert!(member_a.org_visible(None));
    assert!(member_b.org_visible(None));
    // scoped tips only reach the matching active org.
    assert!(member_a.org_visible(Some(org_a)));
    assert!(!member_a.org_visible(Some(org_b)));
    assert!(!member_b.org_visible(Some(org_a)));
}

#[test]
fn platform_admin_holds_every_capability() {
    let ctx = auth_ctx(true, None);
    let caps = ctx.capabilities();
    for cap in runinator_models::capabilities::Capability::ALL {
        assert!(caps.contains(cap), "admin must hold {cap:?}");
    }
}

#[test]
fn disabled_admin_holds_every_capability() {
    let caps = AuthContext::disabled_admin().capabilities();
    assert_eq!(
        caps.len(),
        runinator_models::capabilities::Capability::ALL.len()
    );
}

#[test]
fn org_admin_holds_only_org_capabilities() {
    use runinator_models::capabilities::Capability;
    let ctx = auth_ctx(false, Some(OrgRole::Admin));
    let caps = ctx.capabilities();
    assert!(caps.contains(&Capability::OrgMembersManage));
    assert!(caps.contains(&Capability::OrgNodesScale));
    assert!(!caps.contains(&Capability::UsersManage));
    assert!(!caps.contains(&Capability::SecretsRead));
    assert!(!caps.contains(&Capability::NodesScale));
}

#[test]
fn org_member_holds_no_capabilities() {
    let ctx = auth_ctx(false, Some(OrgRole::Member));
    assert!(ctx.capabilities().is_empty());
}

#[test]
fn require_capability_gates_by_holder() {
    use runinator_models::capabilities::Capability;
    let admin = auth_ctx(true, None);
    let member = auth_ctx(false, Some(OrgRole::Member));
    let org_admin = auth_ctx(false, Some(OrgRole::Admin));

    assert!(admin.require_capability(Capability::UsersManage).is_ok());
    assert!(member.require_capability(Capability::UsersManage).is_err());
    assert!(
        org_admin
            .require_capability(Capability::UsersManage)
            .is_err()
    );
    assert!(
        org_admin
            .require_capability(Capability::OrgMembersManage)
            .is_ok()
    );
}

#[tokio::test]
async fn me_returns_resolved_capabilities() {
    let (db, path) = test_db().await;
    let db = Arc::new(db);
    let user = db
        .create_user("member".into(), None, false, None)
        .await
        .unwrap();
    let user_id = user.id.expect("user id");

    // an org admin sees the org capabilities but none of the platform ones.
    let ctx = AuthContext {
        principal_id: Some(user_id),
        is_admin: false,
        kind: PrincipalKind::User,
        org_id: Some(Uuid::now_v7()),
        org_role: Some(OrgRole::Admin),
    };
    let (status, Json(body)) =
        crate::handlers::auth::me::<SqliteDb>(Extension(db.clone()), Extension(ctx)).await;
    assert_eq!(status, StatusCode::OK);
    let crate::models::ApiResponse::JsonValue(value) = body else {
        panic!("me response must be json");
    };
    let capabilities: Vec<String> = value
        .get("capabilities")
        .and_then(|caps| caps.as_array())
        .expect("capabilities array")
        .iter()
        .filter_map(|cap| cap.as_str().map(str::to_string))
        .collect();
    assert!(capabilities.contains(&"org:members:manage".to_string()));
    assert!(!capabilities.contains(&"users:manage".to_string()));

    let _ = std::fs::remove_file(path);
}
