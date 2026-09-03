//! Hierarchical role assignments, resource ownership, and scoped ACL operations.

use super::*;

fn scope_key(scope: ScopeRef) -> String {
    match scope.id {
        Some(id) => format!("{}:{id}", scope.kind.as_str()),
        None => "platform".to_string(),
    }
}

fn scope_from_key(key: &str) -> Option<ScopeRef> {
    if key == "platform" {
        return Some(ScopeRef::PLATFORM);
    }
    let (kind, id) = key.split_once(':')?;
    let kind = ScopeKind::from_str_lossy(kind)?;
    ScopeRef::new(kind, Uuid::parse_str(id).ok())
}

fn invalid_rbac(message: impl Into<String>) -> SendableError {
    Box::new(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        message.into(),
    ))
}

impl<B> RbacStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn create_service_account(
        &self,
        name: String,
        created_by: Option<Uuid>,
    ) -> Result<ServiceAccount, SendableError> {
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO service_accounts (id, name, disabled, created_by, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?)",
        ))
        .bind(id).bind(&name).bind(false).bind(created_by).bind(now).bind(now)
        .execute(self.pool()).await?;
        Ok(ServiceAccount {
            id,
            name,
            disabled: false,
            created_by,
            created_at: DateTime::from_timestamp(now, 0).unwrap_or_else(Utc::now),
            updated_at: DateTime::from_timestamp(now, 0).unwrap_or_else(Utc::now),
        })
    }

    async fn fetch_service_account(
        &self,
        id: Uuid,
    ) -> Result<Option<ServiceAccount>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, disabled, created_by, created_at, updated_at FROM service_accounts WHERE id = ?",
        )).bind(id).fetch_optional(self.pool()).await?;
        Ok(row.as_ref().map(service_account_from_row::<B>))
    }

    async fn list_service_accounts(&self) -> Result<Vec<ServiceAccount>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, name, disabled, created_by, created_at, updated_at FROM service_accounts ORDER BY name",
        )).fetch_all(self.pool()).await?;
        Ok(rows.iter().map(service_account_from_row::<B>).collect())
    }

    async fn set_service_account_disabled(
        &self,
        id: Uuid,
        disabled: bool,
    ) -> Result<ServiceAccount, SendableError> {
        let current = self
            .fetch_service_account(id)
            .await?
            .ok_or_else(|| invalid_rbac("service account not found"))?;
        let mut tx = self.pool().begin().await?;
        if disabled && !current.disabled {
            if self.dialect() == SqlDialect::Sqlite {
                sqlx::query(&self.render(
                    "UPDATE role_assignments SET updated_at = updated_at WHERE scope_key IN (\
                     SELECT scope_key FROM role_assignments WHERE principal_kind = 'service' AND principal_id = ?)",
                ))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            } else {
                sqlx::query(&self.render(
                    "SELECT principal_id FROM role_assignments WHERE scope_key IN (\
                     SELECT scope_key FROM role_assignments WHERE principal_kind = 'service' AND principal_id = ?) FOR UPDATE",
                ))
                .bind(id)
                .fetch_all(&mut *tx)
                .await?;
            }
            let orphaned_scopes: i64 = sqlx::query_scalar(&self.render(
                "SELECT COUNT(*) FROM role_assignments target WHERE target.principal_kind = 'service' AND target.principal_id = ? \
                 AND ((target.scope_key = 'platform' AND target.role = 'admin') OR (target.scope_key <> 'platform' AND target.role = 'owner')) \
                 AND NOT EXISTS (SELECT 1 FROM role_assignments other WHERE other.scope_key = target.scope_key \
                   AND other.role = target.role \
                   AND (other.principal_kind <> target.principal_kind OR other.principal_id <> target.principal_id) AND (\
                     (other.principal_kind = 'user' AND EXISTS (SELECT 1 FROM users u WHERE u.id = other.principal_id AND u.disabled = ?)) OR \
                     (other.principal_kind = 'service' AND EXISTS (SELECT 1 FROM service_accounts s WHERE s.id = other.principal_id AND s.disabled = ?))))",
            ))
            .bind(id)
            .bind(false)
            .bind(false)
            .fetch_one(&mut *tx)
            .await?;
            if orphaned_scopes > 0 {
                return Err(invalid_rbac(
                    "the last enabled platform administrator or scope owner cannot be disabled",
                ));
            }
        }
        let now = Utc::now().timestamp();
        let result = sqlx::query(
            &self.render("UPDATE service_accounts SET disabled = ?, updated_at = ? WHERE id = ?"),
        )
        .bind(disabled)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        if result.affected() == 0 {
            return Err(invalid_rbac("service account not found"));
        }
        tx.commit().await?;
        Ok(ServiceAccount {
            disabled,
            updated_at: DateTime::from_timestamp(now, 0).unwrap_or_else(Utc::now),
            ..current
        })
    }

    async fn upsert_role_assignment(
        &self,
        principal_kind: runinator_models::auth::PrincipalKind,
        principal_id: Uuid,
        scope: ScopeRef,
        role: Role,
        created_by: Option<Uuid>,
    ) -> Result<RoleAssignment, SendableError> {
        let Some(scope) = ScopeRef::new(scope.kind, scope.id) else {
            return Err(invalid_rbac(
                "platform scopes must have no id and all other scopes require one",
            ));
        };
        let now = Utc::now().timestamp();
        let key = scope_key(scope);
        let mut tx = self.pool().begin().await?;
        if self.dialect() == SqlDialect::Sqlite {
            sqlx::query(
                &self.render(
                    "UPDATE role_assignments SET updated_at = updated_at WHERE scope_key = ?",
                ),
            )
            .bind(&key)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(&self.render(
                "SELECT principal_id FROM role_assignments WHERE scope_key = ? FOR UPDATE",
            ))
            .bind(&key)
            .fetch_all(&mut *tx)
            .await?;
        }
        let current = sqlx::query(&self.render(
            "SELECT role FROM role_assignments WHERE principal_kind = ? AND principal_id = ? AND scope_key = ?",
        ))
        .bind(principal_kind.as_str())
        .bind(principal_id)
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = current {
            let old_role: String = row.try_get("role")?;
            let was_protected = (key == "platform" && old_role == "admin")
                || (key != "platform" && old_role == "owner");
            if was_protected && old_role != role.as_str() {
                let count: i64 = sqlx::query_scalar(&self.render(
                    "SELECT COUNT(*) FROM role_assignments r WHERE scope_key = ? AND role = ? AND (\
                     (r.principal_kind = 'user' AND EXISTS (SELECT 1 FROM users u WHERE u.id = r.principal_id AND u.disabled = ?)) OR \
                     (r.principal_kind = 'service' AND EXISTS (SELECT 1 FROM service_accounts s WHERE s.id = r.principal_id AND s.disabled = ?)))",
                ))
                .bind(&key)
                .bind(&old_role)
                .bind(false)
                .bind(false)
                .fetch_one(&mut *tx)
                .await?;
                if count <= 1 {
                    return Err(invalid_rbac(
                        "the last platform administrator or scope owner cannot be demoted",
                    ));
                }
            }
        }
        let conflict = self.dialect().on_conflict_update(
            "principal_kind, principal_id, scope_key",
            &["role", "created_by", "updated_at"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO role_assignments (principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(principal_kind.as_str())
        .bind(principal_id)
        .bind(&key)
        .bind(role.as_str())
        .bind(created_by)
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let row = sqlx::query(&self.render(
            "SELECT principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at \
             FROM role_assignments WHERE principal_kind = ? AND principal_id = ? AND scope_key = ?",
        ))
        .bind(principal_kind.as_str())
        .bind(principal_id)
        .bind(key)
        .fetch_one(&mut *tx)
        .await?;
        let assignment = role_assignment_from_row::<B>(&row)?;
        tx.commit().await?;
        Ok(assignment)
    }

    async fn delete_role_assignment(
        &self,
        principal_kind: runinator_models::auth::PrincipalKind,
        principal_id: Uuid,
        scope: ScopeRef,
    ) -> Result<(), SendableError> {
        let key = scope_key(scope);
        let mut tx = self.pool().begin().await?;
        if self.dialect() == SqlDialect::Sqlite {
            sqlx::query(
                &self.render(
                    "UPDATE role_assignments SET updated_at = updated_at WHERE scope_key = ?",
                ),
            )
            .bind(&key)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(&self.render(
                "SELECT principal_id FROM role_assignments WHERE scope_key = ? FOR UPDATE",
            ))
            .bind(&key)
            .fetch_all(&mut *tx)
            .await?;
        }
        let current = sqlx::query(&self.render(
            "SELECT role FROM role_assignments WHERE principal_kind = ? AND principal_id = ? AND scope_key = ?",
        ))
        .bind(principal_kind.as_str())
        .bind(principal_id)
        .bind(&key)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = current {
            let role: String = row.try_get("role")?;
            let protected =
                (key == "platform" && role == "admin") || (key != "platform" && role == "owner");
            if protected {
                let count: i64 = sqlx::query_scalar(&self.render(
                    "SELECT COUNT(*) FROM role_assignments r WHERE scope_key = ? AND role = ? AND (\
                     (r.principal_kind = 'user' AND EXISTS (SELECT 1 FROM users u WHERE u.id = r.principal_id AND u.disabled = ?)) OR \
                     (r.principal_kind = 'service' AND EXISTS (SELECT 1 FROM service_accounts s WHERE s.id = r.principal_id AND s.disabled = ?)))",
                ))
                .bind(&key)
                .bind(&role)
                .bind(false)
                .bind(false)
                .fetch_one(&mut *tx)
                .await?;
                if count <= 1 {
                    return Err(invalid_rbac(
                        "the last platform administrator or scope owner cannot be removed",
                    ));
                }
            }
        }
        sqlx::query(&self.render(
            "DELETE FROM role_assignments WHERE principal_kind = ? AND principal_id = ? AND scope_key = ?",
        ))
        .bind(principal_kind.as_str())
        .bind(principal_id)
        .bind(key)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn list_principal_role_assignments(
        &self,
        principal_kind: runinator_models::auth::PrincipalKind,
        principal_id: Uuid,
    ) -> Result<Vec<RoleAssignment>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at \
             FROM role_assignments WHERE principal_kind = ? AND principal_id = ? ORDER BY scope_key",
        ))
        .bind(principal_kind.as_str())
        .bind(principal_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(role_assignment_from_row::<B>).collect()
    }

    async fn list_scope_role_assignments(
        &self,
        scope: ScopeRef,
    ) -> Result<Vec<RoleAssignment>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at \
             FROM role_assignments WHERE scope_key = ? ORDER BY created_at",
        ))
        .bind(scope_key(scope))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(role_assignment_from_row::<B>).collect()
    }

    async fn put_resource_ownership(
        &self,
        ownership: ResourceOwnership,
    ) -> Result<ResourceOwnership, SendableError> {
        if !matches!(
            ownership.tenant.kind,
            ScopeKind::Platform | ScopeKind::Organization
        ) {
            return Err(invalid_rbac(
                "resource tenants must be platform or organization scopes",
            ));
        }
        if ownership.tenant.kind == ScopeKind::Platform
            && ownership.owner.kind != ScopeKind::Platform
        {
            return Err(invalid_rbac(
                "platform resources must remain platform-owned; use a resource grant for individual access",
            ));
        }
        let conflict = self.dialect().on_conflict_update(
            "resource_type, resource_id",
            &[
                "tenant_scope_kind",
                "tenant_scope_id",
                "owner_scope_kind",
                "owner_scope_id",
                "created_by",
                "authz_version",
                "updated_at",
            ],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO resource_ownership (resource_type, resource_id, tenant_scope_kind, tenant_scope_id, owner_scope_kind, owner_scope_id, created_by, authz_version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(ownership.resource_type.as_str())
        .bind(ownership.resource_id)
        .bind(ownership.tenant.kind.as_str())
        .bind(ownership.tenant.id)
        .bind(ownership.owner.kind.as_str())
        .bind(ownership.owner.id)
        .bind(ownership.created_by)
        .bind(ownership.authz_version)
        .bind(ownership.created_at.timestamp())
        .bind(ownership.updated_at.timestamp())
        .execute(self.pool())
        .await?;
        self.fetch_resource_ownership(ownership.resource_type, ownership.resource_id)
            .await?
            .ok_or_else(|| invalid_rbac("resource ownership disappeared after upsert"))
    }

    async fn fetch_resource_ownership(
        &self,
        resource_type: runinator_models::auth::ResourceType,
        resource_id: Uuid,
    ) -> Result<Option<ResourceOwnership>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT resource_type, resource_id, tenant_scope_kind, tenant_scope_id, owner_scope_kind, owner_scope_id, created_by, authz_version, created_at, updated_at \
             FROM resource_ownership WHERE resource_type = ? AND resource_id = ?",
        ))
        .bind(resource_type.as_str())
        .bind(resource_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref()
            .map(resource_ownership_from_row::<B>)
            .transpose()
    }

    async fn list_resource_ownerships(
        &self,
        resource_type: runinator_models::auth::ResourceType,
    ) -> Result<Vec<ResourceOwnership>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT resource_type, resource_id, tenant_scope_kind, tenant_scope_id, owner_scope_kind, owner_scope_id, created_by, authz_version, created_at, updated_at \
             FROM resource_ownership WHERE resource_type = ?",
        ))
        .bind(resource_type.as_str())
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(resource_ownership_from_row::<B>).collect()
    }

    async fn transfer_resource_ownership(
        &self,
        resource_type: runinator_models::auth::ResourceType,
        resource_id: Uuid,
        owner: ScopeRef,
    ) -> Result<ResourceOwnership, SendableError> {
        let mut tx = self.pool().begin().await?;
        let current = sqlx::query(&self.render(
            "SELECT tenant_scope_kind, tenant_scope_id FROM resource_ownership \
             WHERE resource_type = ? AND resource_id = ?",
        ))
        .bind(resource_type.as_str())
        .bind(resource_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| invalid_rbac("resource ownership not found"))?;

        // Moving ownership to an org or an org-scoped team also moves the resource's authoritative
        // tenant. User ownership remains inside the current tenant. Keeping this and the legacy
        // top-level table's org_id in the same transaction prevents ancestry from disagreeing with
        // the row returned by the resource API.
        let (tenant_kind, tenant_id) = match (owner.kind, owner.id) {
            (ScopeKind::Platform, None) => (ScopeKind::Platform, None),
            (ScopeKind::Organization, Some(id)) => (ScopeKind::Organization, Some(id)),
            (ScopeKind::Team, Some(id)) => {
                let team = sqlx::query(
                    &self.render("SELECT scope_kind, scope_id FROM teams WHERE id = ?"),
                )
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .ok_or_else(|| invalid_rbac("target team not found"))?;
                let kind: String = team.try_get("scope_kind")?;
                let scope_id: Option<Uuid> = team.try_get("scope_id")?;
                if kind == ScopeKind::Organization.as_str() {
                    (ScopeKind::Organization, scope_id)
                } else {
                    (ScopeKind::Platform, None)
                }
            }
            (ScopeKind::User, Some(_)) => {
                let kind: String = current.try_get("tenant_scope_kind")?;
                let id: Option<Uuid> = current.try_get("tenant_scope_id")?;
                let kind = ScopeKind::from_str_lossy(&kind)
                    .ok_or_else(|| invalid_rbac("stored resource tenant is invalid"))?;
                (kind, id)
            }
            _ => return Err(invalid_rbac("invalid owner scope")),
        };
        let tenant = ScopeRef::new(tenant_kind, tenant_id)
            .ok_or_else(|| invalid_rbac("target tenant is invalid"))?;
        if tenant.kind == ScopeKind::Platform && owner.kind != ScopeKind::Platform {
            return Err(invalid_rbac(
                "platform resources must remain platform-owned; use a resource grant for individual access",
            ));
        }
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE resource_ownership SET tenant_scope_kind = ?, tenant_scope_id = ?, \
             owner_scope_kind = ?, owner_scope_id = ?, authz_version = authz_version + 1, updated_at = ? \
             WHERE resource_type = ? AND resource_id = ?",
        ))
        .bind(tenant.kind.as_str())
        .bind(tenant.id)
        .bind(owner.kind.as_str())
        .bind(owner.id)
        .bind(now)
        .bind(resource_type.as_str())
        .bind(resource_id)
        .execute(&mut *tx)
        .await?;
        if result.affected() == 0 {
            return Err(invalid_rbac("resource ownership not found"));
        }

        let org_id = (tenant.kind == ScopeKind::Organization)
            .then_some(tenant.id)
            .flatten();
        if matches!(
            resource_type,
            runinator_models::auth::ResourceType::OrchestrationAdapter
                | runinator_models::auth::ResourceType::LibraryFile
        ) && org_id.is_none()
        {
            return Err(invalid_rbac(
                "this resource must remain organization scoped",
            ));
        }
        if resource_type == runinator_models::auth::ResourceType::Setting && org_id.is_none() {
            let row =
                sqlx::query(&self.render("SELECT kind, scope, name FROM settings WHERE id = ?"))
                    .bind(resource_id)
                    .fetch_optional(&mut *tx)
                    .await?
                    .ok_or_else(|| invalid_rbac("setting not found"))?;
            let kind: String = row.try_get("kind")?;
            let scope: String = row.try_get("scope")?;
            let name: String = row.try_get("name")?;
            if !runinator_models::server_settings::is_reserved_server_setting(
                runinator_models::settings::SettingKind::from_str_lossy(&kind),
                &scope,
                &name,
            ) {
                return Err(invalid_rbac(
                    "ordinary settings must remain organization scoped",
                ));
            }
        }
        let table = match resource_type {
            runinator_models::auth::ResourceType::Workflow => "workflows",
            runinator_models::auth::ResourceType::Pipeline => "pipelines",
            runinator_models::auth::ResourceType::FunctionPackage => "function_packages",
            runinator_models::auth::ResourceType::ConsoleSession => "console_sessions",
            runinator_models::auth::ResourceType::Setting => "settings",
            runinator_models::auth::ResourceType::ExecutionProfile => "execution_profiles",
            runinator_models::auth::ResourceType::OrchestrationAdapter => "orchestration_adapters",
            runinator_models::auth::ResourceType::LibraryFile => "workflow_files",
            runinator_models::auth::ResourceType::NotificationPolicy => "notification_policies",
        };
        if resource_type == runinator_models::auth::ResourceType::LibraryFile {
            let owner_id = (owner.kind == ScopeKind::User)
                .then_some(owner.id)
                .flatten();
            sqlx::query(&self.render(
                "UPDATE workflow_files SET org_id = ?, owner_id = ? WHERE id = ? AND scope = 'library'",
            ))
            .bind(org_id)
            .bind(owner_id)
            .bind(resource_id)
            .execute(&mut *tx)
            .await?;
        } else {
            sqlx::query(&self.render(&format!(
                "UPDATE {table} SET org_id = ?, updated_at = ? WHERE id = ?"
            )))
            .bind(org_id)
            .bind(now)
            .bind(resource_id)
            .execute(&mut *tx)
            .await?;
        }

        // `Own` is singular ownership authority, not a permanent share. Remove the old explicit
        // owner grant and materialize a matching one only for a user owner; scoped owners inherit
        // through their org/team/platform role.
        sqlx::query(&self.render(
            "DELETE FROM resource_grants WHERE resource_type = ? AND resource_id = ? AND permission = 'own'",
        ))
        .bind(resource_type.as_str())
        .bind(resource_id)
        .execute(&mut *tx)
        .await?;
        if owner.kind == ScopeKind::User {
            let grant_id = Uuid::now_v7();
            let conflict = self.dialect().on_conflict_update(
                "resource_type, resource_id, principal_type, principal_id",
                &["permission", "created_at"],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO resource_grants (id, resource_type, resource_id, principal_type, principal_id, permission, created_at) \
                 VALUES (?, ?, ?, 'user', ?, 'own', ?) {conflict}"
            )))
            .bind(grant_id)
            .bind(resource_type.as_str())
            .bind(resource_id)
            .bind(owner.id.expect("validated user owner"))
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        let row = sqlx::query(&self.render(
            "SELECT resource_type, resource_id, tenant_scope_kind, tenant_scope_id, owner_scope_kind, owner_scope_id, created_by, authz_version, created_at, updated_at \
             FROM resource_ownership WHERE resource_type = ? AND resource_id = ?",
        ))
        .bind(resource_type.as_str())
        .bind(resource_id)
        .fetch_one(&mut *tx)
        .await?;
        let ownership = resource_ownership_from_row::<B>(&row)?;
        tx.commit().await?;
        Ok(ownership)
    }

    async fn revoke_scoped_grant(
        &self,
        resource_type: runinator_models::auth::ResourceType,
        resource_id: Uuid,
        grant_id: Uuid,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "DELETE FROM resource_grants WHERE id = ? AND resource_type = ? AND resource_id = ?",
        ))
        .bind(grant_id)
        .bind(resource_type.as_str())
        .bind(resource_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn list_effective_resource_grants(
        &self,
        resource_type: runinator_models::auth::ResourceType,
        resource_id: Uuid,
        principal_id: Uuid,
    ) -> Result<Vec<Grant>, SendableError> {
        let memberships = sqlx::query(&self.render(
            "SELECT scope_key FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ?",
        ))
        .bind(principal_id)
        .fetch_all(self.pool()).await?;
        let team_ids: Vec<Uuid> = memberships
            .iter()
            .filter_map(|row| {
                scope_from_key(&row.get::<String, _>("scope_key")).and_then(|scope| {
                    (scope.kind == ScopeKind::Team)
                        .then_some(scope.id)
                        .flatten()
                })
            })
            .collect();
        let rows = sqlx::query(&self.render(
            "SELECT g.id, g.resource_type, g.resource_id, g.principal_type, g.principal_id, g.permission, g.created_at \
             FROM resource_grants g WHERE g.resource_type = ? AND g.resource_id = ?",
        ))
        .bind(resource_type.as_str())
        .bind(resource_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_grant)
            .filter(|grant| {
                (grant.principal_type == runinator_models::auth::PrincipalType::User
                    && grant.principal_id == principal_id)
                    || (grant.principal_type == runinator_models::auth::PrincipalType::Team
                        && team_ids.contains(&grant.principal_id))
            })
            .collect())
    }
}

fn service_account_from_row<B: SqlBackend>(row: &<B::Db as Database>::Row) -> ServiceAccount
where
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
{
    let created = row.get::<i64, _>("created_at");
    let updated = row.get::<i64, _>("updated_at");
    ServiceAccount {
        id: row.get("id"),
        name: row.get("name"),
        disabled: row.get("disabled"),
        created_by: row.get("created_by"),
        created_at: DateTime::from_timestamp(created, 0).unwrap_or_else(Utc::now),
        updated_at: DateTime::from_timestamp(updated, 0).unwrap_or_else(Utc::now),
    }
}

fn role_assignment_from_row<B: SqlBackend>(
    row: &<B::Db as Database>::Row,
) -> Result<RoleAssignment, SendableError>
where
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
{
    let principal_kind = runinator_models::auth::PrincipalKind::from_str_lossy(
        &row.get::<String, _>("principal_kind"),
    )
    .ok_or_else(|| invalid_rbac("invalid principal kind"))?;
    let scope = scope_from_key(&row.get::<String, _>("scope_key"))
        .ok_or_else(|| invalid_rbac("invalid scope key"))?;
    let role = Role::from_parts(scope.kind.as_str(), &row.get::<String, _>("role"))
        .ok_or_else(|| invalid_rbac("invalid role"))?;
    let created = row.get::<i64, _>("created_at");
    let updated = row.get::<i64, _>("updated_at");
    Ok(RoleAssignment {
        principal_kind,
        principal_id: row.get("principal_id"),
        scope,
        role,
        created_by: row.get("created_by"),
        created_at: DateTime::from_timestamp(created, 0)
            .ok_or_else(|| invalid_rbac("invalid created_at"))?,
        updated_at: DateTime::from_timestamp(updated, 0)
            .ok_or_else(|| invalid_rbac("invalid updated_at"))?,
    })
}

fn resource_ownership_from_row<B: SqlBackend>(
    row: &<B::Db as Database>::Row,
) -> Result<ResourceOwnership, SendableError>
where
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
{
    let resource_type = runinator_models::auth::ResourceType::from_str_lossy(
        &row.get::<String, _>("resource_type"),
    )
    .ok_or_else(|| invalid_rbac("invalid resource type"))?;
    let tenant_kind = ScopeKind::from_str_lossy(&row.get::<String, _>("tenant_scope_kind"))
        .ok_or_else(|| invalid_rbac("invalid tenant scope"))?;
    let owner_kind = ScopeKind::from_str_lossy(&row.get::<String, _>("owner_scope_kind"))
        .ok_or_else(|| invalid_rbac("invalid owner scope"))?;
    let tenant = ScopeRef::new(tenant_kind, row.get("tenant_scope_id"))
        .ok_or_else(|| invalid_rbac("invalid tenant reference"))?;
    let owner = ScopeRef::new(owner_kind, row.get("owner_scope_id"))
        .ok_or_else(|| invalid_rbac("invalid owner reference"))?;
    let created = row.get::<i64, _>("created_at");
    let updated = row.get::<i64, _>("updated_at");
    Ok(ResourceOwnership {
        resource_type,
        resource_id: row.get("resource_id"),
        tenant,
        owner,
        created_by: row.get("created_by"),
        authz_version: row.get("authz_version"),
        created_at: DateTime::from_timestamp(created, 0)
            .ok_or_else(|| invalid_rbac("invalid created_at"))?,
        updated_at: DateTime::from_timestamp(updated, 0)
            .ok_or_else(|| invalid_rbac("invalid updated_at"))?,
    })
}
