//! Test identity rows, memberships, and the API key lifecycle.

use super::*;

#[tokio::test]
async fn reusable_dependencies_follow_owner_membership_grants_and_tenant_boundaries() {
    let path = std::env::temp_dir().join(format!(
        "runinator-resource-dependencies-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let org_id = db
        .create_org("Dependency Org".into(), "dependency-org".into())
        .await
        .unwrap()
        .id
        .unwrap();
    let other_org_id = db
        .create_org("Other Dependency Org".into(), "other-dependency-org".into())
        .await
        .unwrap()
        .id
        .unwrap();
    let tenant = ScopeRef::new(
        runinator_models::rbac::ScopeKind::Organization,
        Some(org_id),
    )
    .unwrap();
    let member = db
        .create_user("dependency-member".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();
    let outsider = db
        .create_user("dependency-outsider".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();
    db.add_org_member(org_id, member, OrgRole::Member)
        .await
        .unwrap();
    db.add_org_member(org_id, outsider, OrgRole::Member)
        .await
        .unwrap();
    let team = db
        .create_team("dependency-team".into(), tenant)
        .await
        .unwrap();
    let team_id = team.id.unwrap();
    db.add_team_member(team_id, member, TeamRole::Member)
        .await
        .unwrap();

    let dependency_id = Uuid::now_v7();
    let now = Utc::now();
    db.put_resource_ownership(ResourceOwnership {
        resource_type: ResourceType::Setting,
        resource_id: dependency_id,
        tenant,
        owner: ScopeRef::new(runinator_models::rbac::ScopeKind::Team, Some(team_id)).unwrap(),
        created_by: Some(member),
        authz_version: 1,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();

    assert!(
        runinator_store::resource_access::owner_can_consume(
            &db,
            ScopeRef::new(runinator_models::rbac::ScopeKind::User, Some(member)).unwrap(),
            tenant,
            ResourceType::Setting,
            dependency_id,
        )
        .await
        .unwrap()
    );
    assert!(
        !runinator_store::resource_access::owner_can_consume(
            &db,
            ScopeRef::new(runinator_models::rbac::ScopeKind::User, Some(outsider)).unwrap(),
            tenant,
            ResourceType::Setting,
            dependency_id,
        )
        .await
        .unwrap()
    );

    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Setting,
        resource_id: dependency_id,
        principal_type: PrincipalType::User,
        principal_id: outsider,
        permission: Permission::Run,
        created_at: now,
    })
    .await
    .unwrap();
    assert!(
        runinator_store::resource_access::owner_can_consume(
            &db,
            ScopeRef::new(runinator_models::rbac::ScopeKind::User, Some(outsider)).unwrap(),
            tenant,
            ResourceType::Setting,
            dependency_id,
        )
        .await
        .unwrap()
    );
    assert!(
        !runinator_store::resource_access::owner_can_consume(
            &db,
            ScopeRef::new(runinator_models::rbac::ScopeKind::User, Some(outsider)).unwrap(),
            ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(other_org_id),
            )
            .unwrap(),
            ResourceType::Setting,
            dependency_id,
        )
        .await
        .unwrap()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn users_grants_and_teams_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-authz-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    // a user with a local password is resolvable by username, carrying its stored hash.
    let user = db
        .create_user(
            "alice".into(),
            Some("a@x.io".into()),
            Some("argon-hash".into()),
        )
        .await
        .unwrap();
    let user_id = user.id.unwrap();
    let credential = db
        .fetch_local_credential("alice".into())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(credential.password_hash, "argon-hash");
    assert_eq!(db.count_users().await.unwrap(), 1);

    // a direct user grant on a workflow is listed for the resource and for the user.
    let workflow_id = Uuid::now_v7();
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::Edit,
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let grants = db
        .list_grants("workflow".into(), workflow_id)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].permission, Permission::Edit);
    let user_grants = db
        .list_user_grants("workflow".into(), user_id)
        .await
        .unwrap();
    assert_eq!(user_grants.len(), 1);

    // upsert: re-granting the same (resource, principal) updates the permission in place.
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::Own,
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let grants = db
        .list_grants("workflow".into(), workflow_id)
        .await
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert_eq!(grants[0].permission, Permission::Own);

    // teams: membership feeds team-scoped grants.
    let team = db
        .create_team("ops".into(), ScopeRef::PLATFORM)
        .await
        .unwrap();
    let team_id = team.id.unwrap();
    let updated_team = db.update_team(team_id, "platform".into()).await.unwrap();
    assert_eq!(updated_team.name, "platform");
    db.add_team_member(team_id, user_id, TeamRole::Member)
        .await
        .unwrap();
    db.add_team_member(team_id, user_id, TeamRole::Member)
        .await
        .unwrap(); // idempotent
    assert_eq!(db.list_user_team_ids(user_id).await.unwrap(), vec![team_id]);
    assert_eq!(
        db.list_user_teams(user_id).await.unwrap()[0].name,
        "platform"
    );
    assert_eq!(
        db.list_team_members(team_id).await.unwrap()[0].username,
        "alice"
    );
    db.create_grant(Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::Team,
        principal_id: team_id,
        permission: Permission::Run,
        created_at: Utc::now(),
    })
    .await
    .unwrap();
    let team_grants = db
        .list_team_grants("workflow".into(), team_id)
        .await
        .unwrap();
    assert_eq!(team_grants.len(), 1);
    assert_eq!(team_grants[0].permission, Permission::Run);
}

#[tokio::test]
async fn orgs_and_memberships_round_trip() {
    let path = std::env::temp_dir().join(format!(
        "runinator-orgs-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let acme = db.create_org("Acme".into(), "acme".into()).await.unwrap();
    let acme_id = acme.id.unwrap();
    // slug is the unique routing identifier.
    assert!(db.fetch_org_by_slug("acme".into()).await.unwrap().is_some());
    assert_eq!(db.list_orgs().await.unwrap().len(), 1);

    let user = db.create_user("bob".into(), None, None).await.unwrap();
    let user_id = user.id.unwrap();

    // membership is idempotent on (org, user); re-adding updates the role in place.
    db.add_org_member(acme_id, user_id, OrgRole::Member)
        .await
        .unwrap();
    db.add_org_member(acme_id, user_id, OrgRole::Admin)
        .await
        .unwrap();
    let membership = db
        .fetch_org_membership(acme_id, user_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(membership.role, OrgRole::Admin);
    assert_eq!(db.list_org_members(acme_id).await.unwrap().len(), 1);

    // the user's org list carries their role in each org.
    let user_orgs = db.list_user_orgs(user_id).await.unwrap();
    assert_eq!(user_orgs.len(), 1);
    assert_eq!(user_orgs[0].1, OrgRole::Admin);

    // update flips the disabled flag and rename; slug is immutable.
    let updated = db
        .update_org(acme_id, Some("Acme Inc".into()), Some(true))
        .await
        .unwrap();
    assert_eq!(updated.name, "Acme Inc");
    assert!(updated.disabled);
    assert_eq!(updated.slug, "acme");

    // removing the member empties the roster; deleting the org clears everything.
    db.remove_org_member(acme_id, user_id).await.unwrap();
    assert!(db.list_org_members(acme_id).await.unwrap().is_empty());
    db.delete_org(acme_id).await.unwrap();
    assert!(db.fetch_org(acme_id).await.unwrap().is_none());
}

#[tokio::test]
async fn api_keys_support_admin_lookup_update_and_revoke() {
    let path = std::env::temp_dir().join(format!(
        "runinator-api-keys-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let user = db.create_user("api-user".into(), None, None).await.unwrap();
    let user_id = user.id.unwrap();
    let key_id = Uuid::now_v7();
    let expires_at = Utc::now() + Duration::days(1);
    let key = db
        .create_api_key(ApiKeyRecord {
            key: ApiKey {
                id: Some(key_id),
                name: "initial".into(),
                principal_kind: runinator_models::auth::PrincipalKind::User,
                principal_id: user_id,
                system_role: None,
                org_id: None,
                action_ceiling: Vec::new(),
                key_prefix: "testprefix".into(),
                last_used_at: None,
                expires_at: Some(expires_at),
                disabled: false,
                created_at: Utc::now(),
            },
            key_hash: "hash".into(),
        })
        .await
        .unwrap();
    assert_eq!(key.id, Some(key_id));

    let fetched = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert_eq!(fetched.key.name, "initial");
    assert_eq!(fetched.key_hash, "hash");

    let updated = db
        .update_api_key(key_id, Some("renamed".into()), Some(None), Some(true))
        .await
        .unwrap();
    assert_eq!(updated.name, "renamed");
    assert_eq!(updated.expires_at, None);
    assert!(updated.disabled);

    db.revoke_api_key(key_id).await.unwrap();
    let revoked = db.fetch_api_key(key_id).await.unwrap().unwrap();
    assert!(revoked.key.disabled);

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn hierarchical_assignments_ownership_and_scoped_grants_are_enforced() {
    let path = std::env::temp_dir().join(format!(
        "runinator-rbac-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();
    let user_id = db
        .create_user("owner".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();

    db.upsert_role_assignment(
        PrincipalKind::User,
        user_id,
        ScopeRef::PLATFORM,
        Role::Platform(PlatformRole::Admin),
        None,
    )
    .await
    .unwrap();
    assert!(
        db.delete_role_assignment(PrincipalKind::User, user_id, ScopeRef::PLATFORM)
            .await
            .is_err()
    );

    let service = db
        .create_service_account("control-plane".into(), Some(user_id))
        .await
        .unwrap();
    db.upsert_role_assignment(
        PrincipalKind::Service,
        service.id,
        ScopeRef::PLATFORM,
        Role::Platform(PlatformRole::Admin),
        Some(user_id),
    )
    .await
    .unwrap();
    assert!(
        db.delete_role_assignment(PrincipalKind::User, user_id, ScopeRef::PLATFORM)
            .await
            .is_err()
    );
    db.set_service_account_disabled(service.id, true)
        .await
        .unwrap();

    let resource_id = Uuid::now_v7();
    let now = Utc::now();
    assert!(
        db.put_resource_ownership(ResourceOwnership {
            resource_type: ResourceType::ConsoleSession,
            resource_id,
            tenant: ScopeRef::PLATFORM,
            owner: ScopeRef::new(runinator_models::rbac::ScopeKind::User, Some(user_id)).unwrap(),
            created_by: Some(user_id),
            authz_version: 1,
            created_at: now,
            updated_at: now,
        })
        .await
        .is_err()
    );
    db.put_resource_ownership(ResourceOwnership {
        resource_type: ResourceType::ConsoleSession,
        resource_id,
        tenant: ScopeRef::PLATFORM,
        owner: ScopeRef::PLATFORM,
        created_by: Some(user_id),
        authz_version: 1,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();
    let ownership = db
        .fetch_resource_ownership(ResourceType::ConsoleSession, resource_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ownership.owner, ScopeRef::PLATFORM);

    let other_resource_id = Uuid::now_v7();
    let grant = db
        .create_grant(Grant {
            id: None,
            resource_type: ResourceType::ConsoleSession,
            resource_id,
            principal_type: PrincipalType::User,
            principal_id: user_id,
            permission: Permission::Edit,
            created_at: now,
        })
        .await
        .unwrap();
    assert!(
        !db.revoke_scoped_grant(
            ResourceType::ConsoleSession,
            other_resource_id,
            grant.id.unwrap(),
        )
        .await
        .unwrap()
    );
    assert!(
        db.revoke_scoped_grant(ResourceType::ConsoleSession, resource_id, grant.id.unwrap(),)
            .await
            .unwrap()
    );

    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn team_names_and_memberships_are_tenant_bounded() {
    let path = std::env::temp_dir().join(format!(
        "runinator-team-tenancy-{}.db",
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    let db = SqliteDb::new(path.to_str().unwrap()).await.unwrap();
    db.run_init_scripts(&Vec::new()).await.unwrap();

    let member = db
        .create_user("tenant-team-member".into(), None, None)
        .await
        .unwrap()
        .id
        .unwrap();
    let first_org = db
        .create_org("First Teams".into(), "first-teams".into())
        .await
        .unwrap()
        .id
        .unwrap();
    let second_org = db
        .create_org("Second Teams".into(), "second-teams".into())
        .await
        .unwrap()
        .id
        .unwrap();
    let first_scope = ScopeRef::new(
        runinator_models::rbac::ScopeKind::Organization,
        Some(first_org),
    )
    .unwrap();
    let second_scope = ScopeRef::new(
        runinator_models::rbac::ScopeKind::Organization,
        Some(second_org),
    )
    .unwrap();

    let first_team = db
        .create_team("operators".into(), first_scope)
        .await
        .unwrap();
    db.create_team("operators".into(), second_scope)
        .await
        .unwrap();
    assert!(
        db.create_team("operators".into(), first_scope)
            .await
            .is_err()
    );

    db.add_org_member(first_org, member, OrgRole::Member)
        .await
        .unwrap();
    db.add_team_member(first_team.id.unwrap(), member, TeamRole::Member)
        .await
        .unwrap();

    db.upsert_setting(
        Some(first_org),
        SettingKind::Config,
        "team-boundary".into(),
        "retired-setting".into(),
        b"value".to_vec(),
        Utc::now().timestamp(),
    )
    .await
    .unwrap();
    let setting = db
        .fetch_setting(
            Some(first_org),
            SettingKind::Config,
            "team-boundary".into(),
            "retired-setting".into(),
        )
        .await
        .unwrap()
        .unwrap();
    let setting_id = setting.id;
    let now = Utc::now();
    db.put_resource_ownership(ResourceOwnership {
        resource_type: ResourceType::Setting,
        resource_id: setting_id,
        tenant: first_scope,
        owner: ScopeRef::new(runinator_models::rbac::ScopeKind::Team, first_team.id).unwrap(),
        created_by: Some(member),
        authz_version: 1,
        created_at: now,
        updated_at: now,
    })
    .await
    .unwrap();
    assert!(db.delete_team(first_team.id.unwrap()).await.is_err());
    db.delete_setting(
        Some(first_org),
        SettingKind::Config,
        "team-boundary".into(),
        "retired-setting".into(),
    )
    .await
    .unwrap();
    assert!(
        db.fetch_resource_ownership(ResourceType::Setting, setting_id)
            .await
            .unwrap()
            .is_none()
    );
    db.delete_team(first_team.id.unwrap()).await.unwrap();

    db.remove_org_member(first_org, member).await.unwrap();
    assert!(db.list_user_team_ids(member).await.unwrap().is_empty());

    let _ = std::fs::remove_file(path);
}
