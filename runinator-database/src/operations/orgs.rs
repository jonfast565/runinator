//! organizations, quotas, and usage.
//!
//! the `OrgStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> OrgStore for SqlStore<B>
where
    B: SqlBackend,
    // encode bounds for every bound value type.
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Vec<u8>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    // decode bounds (operations read a couple of columns directly; mappers read the rest).
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    // row indexing + executor plumbing.
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn reconcile_platform_organization(&self) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        let lock = if self.dialect() == SqlDialect::Sqlite {
            ""
        } else {
            " FOR UPDATE"
        };
        if self.dialect() == SqlDialect::Sqlite {
            sqlx::query("UPDATE organizations SET updated_at = updated_at WHERE slug = 'platform'")
                .execute(&mut *tx)
                .await?;
        }
        let row = sqlx::query(&format!(
            "SELECT id FROM organizations WHERE slug = 'platform'{lock}"
        ))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            return Ok(());
        };
        let id: Uuid = row.try_get("id")?;
        let now = Utc::now().timestamp();
        for (table, column) in [
            ("teams", "scope_id"),
            ("orchestration_adapters", "org_id"),
            ("workflow_files", "org_id"),
            ("settings", "org_id"),
            ("org_resource_groups", "org_id"),
            ("org_usage_ledger", "org_id"),
            ("api_keys", "org_id"),
            ("agent_enrollment_tokens", "org_id"),
            ("calendar_subscriptions", "scope_id"),
            ("broker_ingress_messages", "scope_id"),
            ("ingress_control_gates", "owner_scope_id"),
            ("ingress_control_events", "owner_scope_id"),
            ("replicas", "registered_by_org_id"),
            ("freeze_windows", "org_id"),
        ] {
            let count: i64 = sqlx::query_scalar(
                &self.render(&format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?")),
            )
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if count > 0 {
                return Err(crate::errors::PLATFORM_RECONCILIATION_BLOCKED.error(table));
            }
        }
        let active: i64 = sqlx::query_scalar(&self.render(
            "SELECT COUNT(*) FROM broker_ingress_sessions WHERE scope_id = ? AND (expires_at IS NULL OR expires_at > ?)",
        )).bind(id).bind(now).fetch_one(&mut *tx).await?;
        if active > 0 {
            return Err(
                crate::errors::PLATFORM_RECONCILIATION_BLOCKED.error("active ingress session")
            );
        }
        let unsupported: i64 = sqlx::query_scalar(&self.render(
            "SELECT COUNT(*) FROM resource_ownership WHERE tenant_scope_id = ? AND resource_type IN ('setting', 'orchestration_adapter', 'library_file')",
        )).bind(id).fetch_one(&mut *tx).await?;
        if unsupported > 0 {
            return Err(
                crate::errors::PLATFORM_RECONCILIATION_BLOCKED.error("organization-only ownership")
            );
        }
        for (kind, table) in [
            ("workflow", "workflows"),
            ("pipeline", "pipelines"),
            ("console_session", "console_sessions"),
            ("function_package", "function_packages"),
            ("execution_profile", "execution_profiles"),
            ("notification_policy", "notification_policies"),
        ] {
            let rows = sqlx::query(&self.render(&format!(
                "SELECT id FROM {table} WHERE org_id = ? UNION SELECT resource_id AS id FROM resource_ownership WHERE resource_type = ? AND tenant_scope_id = ?"
            ))).bind(id).bind(kind).bind(id).fetch_all(&mut *tx).await?;
            for row in rows {
                let resource_id: Uuid = row.try_get("id")?;
                let conflicts: i64 = sqlx::query_scalar(&self.render(&format!(
                    "SELECT COUNT(*) FROM {table} WHERE id = ? AND org_id IS NOT NULL AND org_id <> ?"
                ))).bind(resource_id).bind(id).fetch_one(&mut *tx).await?;
                if conflicts > 0 {
                    return Err(crate::errors::PLATFORM_RECONCILIATION_BLOCKED
                        .error("conflicting source organization"));
                }
                let conflicts: i64 = sqlx::query_scalar(&self.render(
                    "SELECT COUNT(*) FROM resource_ownership WHERE resource_type = ? AND resource_id = ? AND tenant_scope_id IS NOT NULL AND tenant_scope_id <> ?"
                )).bind(kind).bind(resource_id).bind(id).fetch_one(&mut *tx).await?;
                if conflicts > 0 {
                    return Err(crate::errors::PLATFORM_RECONCILIATION_BLOCKED
                        .error("conflicting resource tenant"));
                }
                let exists: i64 = sqlx::query_scalar(
                    &self.render(&format!("SELECT COUNT(*) FROM {table} WHERE id = ?")),
                )
                .bind(resource_id)
                .fetch_one(&mut *tx)
                .await?;
                sqlx::query(&self.render(
                    "DELETE FROM resource_grants WHERE resource_type = ? AND resource_id = ?",
                ))
                .bind(kind)
                .bind(resource_id)
                .execute(&mut *tx)
                .await?;
                if exists == 0 {
                    sqlx::query(&self.render("DELETE FROM resource_ownership WHERE resource_type = ? AND resource_id = ?"))
                        .bind(kind).bind(resource_id).execute(&mut *tx).await?;
                    continue;
                }
                sqlx::query(&self.render(&format!(
                    "UPDATE {table} SET org_id = NULL, updated_at = ? WHERE id = ?"
                )))
                .bind(now)
                .bind(resource_id)
                .execute(&mut *tx)
                .await?;
                let updated = sqlx::query(&self.render(
                    "UPDATE resource_ownership SET tenant_scope_kind = 'platform', tenant_scope_id = NULL, owner_scope_kind = 'platform', owner_scope_id = NULL, authz_version = authz_version + 1, updated_at = ? WHERE resource_type = ? AND resource_id = ?"
                )).bind(now).bind(kind).bind(resource_id).execute(&mut *tx).await?;
                if updated.affected() == 0 {
                    sqlx::query(&self.render(
                        "INSERT INTO resource_ownership (resource_type, resource_id, tenant_scope_kind, tenant_scope_id, owner_scope_kind, owner_scope_id, created_by, authz_version, created_at, updated_at) VALUES (?, ?, 'platform', NULL, 'platform', NULL, NULL, 1, ?, ?)"
                    )).bind(kind).bind(resource_id).bind(now).bind(now).execute(&mut *tx).await?;
                }
            }
        }
        let remaining: i64 = sqlx::query_scalar(&self.render(
            "SELECT COUNT(*) FROM resource_ownership WHERE tenant_scope_id = ? OR owner_scope_id = ?",
        )).bind(id).bind(id).fetch_one(&mut *tx).await?;
        if remaining > 0 {
            return Err(crate::errors::PLATFORM_RECONCILIATION_BLOCKED
                .error("unsupported ownership reference"));
        }
        for table in ["notifications"] {
            sqlx::query(&self.render(&format!(
                "UPDATE {table} SET org_id = NULL WHERE org_id = ?"
            )))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&self.render("DELETE FROM broker_ingress_sessions WHERE scope_id = ?"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("DELETE FROM role_assignments WHERE scope_key = ?"))
            .bind(format!("organization:{id}"))
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("DELETE FROM organizations WHERE id = ?"))
            .bind(id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn create_org(&self, name: String, slug: String) -> Result<Organization, SendableError> {
        if runinator_models::orgs::slugify(&slug) == "platform" {
            return Err(crate::errors::RESERVED_PLATFORM_ORGANIZATION.bare());
        }
        let id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO organizations (id, name, slug, disabled, max_nodes_json, max_monthly_cents, created_at, updated_at) \
             VALUES (?, ?, ?, ?, '{}', 0, ?, ?)",
        ))
        .bind(id)
        .bind(&name)
        .bind(&slug)
        .bind(false)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;
        let created = DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now);
        Ok(Organization {
            id: Some(id),
            name,
            slug,
            disabled: false,
            created_at: created,
            updated_at: created,
        })
    }

    async fn fetch_org_by_slug(&self, slug: String) -> Result<Option<Organization>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id, name, slug, disabled, created_at, updated_at FROM organizations WHERE slug = ?",
        ))
        .bind(&slug)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_organization))
    }

    async fn list_orgs(&self) -> Result<Vec<Organization>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id, name, slug, disabled, created_at, updated_at FROM organizations ORDER BY name",
        ))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_organization).collect())
    }

    async fn update_org(
        &self,
        id: Uuid,
        name: Option<String>,
        disabled: Option<bool>,
    ) -> Result<Organization, SendableError> {
        let Some(current) = self.fetch_org(id).await? else {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Organization {id} not found"),
            )));
        };
        let name = name.unwrap_or(current.name);
        let disabled = disabled.unwrap_or(current.disabled);
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE organizations SET name = ?, disabled = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(&name)
        .bind(disabled)
        .bind(now)
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(Organization {
            id: Some(id),
            name,
            slug: current.slug,
            disabled,
            created_at: current.created_at,
            updated_at: DateTime::<Utc>::from_timestamp(now, 0).unwrap_or_else(Utc::now),
        })
    }

    async fn delete_org(&self, id: Uuid) -> Result<(), SendableError> {
        retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            let team_count: i64 = sqlx::query_scalar(&self.render(
                "SELECT COUNT(*) FROM teams WHERE scope_kind = 'organization' AND scope_id = ?",
            ))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if team_count > 0 {
                return Err(sqlx::Error::Protocol(
                    "delete organization teams before deleting the organization".to_string(),
                ));
            }
            let resource_count: i64 = sqlx::query_scalar(&self.render(
                "SELECT COUNT(*) FROM resource_ownership WHERE tenant_scope_kind = 'organization' AND tenant_scope_id = ?",
            ))
            .bind(id)
            .fetch_one(&mut *tx)
            .await?;
            if resource_count > 0 {
                return Err(sqlx::Error::Protocol(
                    "transfer or delete organization resources before deleting the organization"
                        .to_string(),
                ));
            }
            sqlx::query(&self.render("DELETE FROM role_assignments WHERE scope_key = ?"))
                .bind(format!("organization:{id}"))
                .execute(&mut *tx)
                .await?;
            sqlx::query(&self.render("DELETE FROM organizations WHERE id = ?"))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await
        })
        .await?;
        Ok(())
    }

    async fn add_org_member(
        &self,
        org_id: Uuid,
        user_id: Uuid,
        role: OrgRole,
    ) -> Result<(), SendableError> {
        let user_count: i64 = sqlx::query_scalar(
            &self.render("SELECT COUNT(*) FROM users WHERE id = ? AND disabled = ?"),
        )
        .bind(user_id)
        .bind(false)
        .fetch_one(self.pool())
        .await?;
        if user_count == 0 {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "organization member does not exist or is disabled",
            )));
        }
        let org_count: i64 = sqlx::query_scalar(
            &self.render("SELECT COUNT(*) FROM organizations WHERE id = ? AND disabled = ?"),
        )
        .bind(org_id)
        .bind(false)
        .fetch_one(self.pool())
        .await?;
        if org_count == 0 {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "organization does not exist or is disabled",
            )));
        }
        let now = Utc::now().timestamp();
        let key = format!("organization:{org_id}");
        let conflict = self.dialect().on_conflict_update(
            "principal_kind, principal_id, scope_key",
            &["role", "updated_at"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO role_assignments (principal_kind, principal_id, scope_key, role, created_by, created_at, updated_at) \
             VALUES ('user', ?, ?, ?, NULL, ?, ?) {conflict}",
        )))
        .bind(user_id).bind(key).bind(role.as_str()).bind(now).bind(now)
        .execute(self.pool()).await?;
        Ok(())
    }

    async fn remove_org_member(&self, org_id: Uuid, user_id: Uuid) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        let owned_resources: i64 = sqlx::query_scalar(&self.render(
            "SELECT COUNT(*) FROM resource_ownership WHERE tenant_scope_kind = 'organization' AND tenant_scope_id = ? AND owner_scope_kind = 'user' AND owner_scope_id = ?",
        ))
        .bind(org_id)
        .bind(user_id)
        .fetch_one(&mut *tx)
        .await?;
        if owned_resources > 0 {
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "transfer resources owned by this user before removing them from the organization",
            )));
        }
        let teams = sqlx::query(
            &self.render("SELECT id FROM teams WHERE scope_kind = 'organization' AND scope_id = ?"),
        )
        .bind(org_id)
        .fetch_all(&mut *tx)
        .await?;
        for team in teams {
            let team_id: Uuid = team.try_get("id")?;
            sqlx::query(&self.render(
                "DELETE FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ? AND scope_key = ?",
            ))
            .bind(user_id)
            .bind(format!("team:{team_id}"))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&self.render(
            "DELETE FROM resource_grants WHERE principal_type = 'user' AND principal_id = ? AND (resource_type, resource_id) IN (SELECT resource_type, resource_id FROM resource_ownership WHERE tenant_scope_kind = 'organization' AND tenant_scope_id = ?)",
        ))
        .bind(user_id)
        .bind(org_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query(&self.render(
            "DELETE FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ? AND scope_key = ?",
        ))
        .bind(user_id)
        .bind(format!("organization:{org_id}"))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn fetch_org_membership(
        &self,
        org_id: Uuid,
        user_id: Uuid,
    ) -> Result<Option<OrgMembership>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT ? AS org_id, principal_id AS user_id, role, created_at FROM role_assignments \
             WHERE principal_kind = 'user' AND principal_id = ? AND scope_key = ?",
        ))
        .bind(org_id)
        .bind(user_id)
        .bind(format!("organization:{org_id}"))
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_org_membership))
    }

    async fn list_org_members(&self, org_id: Uuid) -> Result<Vec<OrgMembership>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT ? AS org_id, principal_id AS user_id, role, created_at FROM role_assignments \
             WHERE principal_kind = 'user' AND scope_key = ?",
        ))
        .bind(org_id)
        .bind(format!("organization:{org_id}"))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_org_membership).collect())
    }

    async fn list_user_orgs(
        &self,
        user_id: Uuid,
    ) -> Result<Vec<(Organization, OrgRole)>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT scope_key, role FROM role_assignments \
             WHERE principal_kind = 'user' AND principal_id = ? AND scope_key <> 'platform' \
             ORDER BY scope_key",
        ))
        .bind(user_id)
        .fetch_all(self.pool())
        .await?;
        let mut memberships = Vec::new();
        for row in rows {
            let key = row.get::<String, _>("scope_key");
            let Some(raw_id) = key.strip_prefix("organization:") else {
                continue;
            };
            let Ok(org_id) = Uuid::parse_str(raw_id) else {
                continue;
            };
            let Some(org) = self.fetch_org(org_id).await? else {
                continue;
            };
            let role =
                OrgRole::from_str_lossy(&row.get::<String, _>("role")).unwrap_or(OrgRole::Member);
            memberships.push((org, role));
        }
        memberships.sort_by(|left, right| left.0.name.cmp(&right.0.name));
        Ok(memberships)
    }

    async fn fetch_org_quota(&self, org_id: Uuid) -> Result<Option<OrgQuota>, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT id AS org_id, max_nodes_json, max_monthly_cents FROM organizations WHERE id = ?",
        ))
        .bind(org_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_org_quota))
    }

    async fn upsert_org_quota(&self, quota: OrgQuota) -> Result<OrgQuota, SendableError> {
        let now = Utc::now().timestamp();
        let max_nodes_json = serde_json::to_string(&quota.max_nodes_per_kind)?;
        sqlx::query(&self.render(
            "UPDATE organizations SET max_nodes_json = ?, max_monthly_cents = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(&max_nodes_json)
        .bind(quota.max_monthly_cents as i64)
        .bind(now)
        .bind(quota.org_id)
        .execute(self.pool())
        .await?;
        Ok(quota)
    }

    async fn insert_usage_sample(&self, sample: UsageSample) -> Result<(), SendableError> {
        // idempotent per (org, backend, kind, sampled_at): the sampler buckets sampled_at to the
        // interval boundary. Any number of WS replicas or engine workers can sample the same
        // window converge to one row instead of over-counting node-hours by the instance count.
        let conflict = self
            .dialect()
            .on_conflict_nothing("org_id, backend, kind, sampled_at", "node_count");
        sqlx::query(&self.render(&format!(
            "INSERT INTO org_usage_ledger (id, org_id, backend, kind, node_count, sampled_at) \
             VALUES (?, ?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(Uuid::now_v7())
        .bind(sample.org_id)
        .bind(sample.backend.as_str())
        .bind(sample.kind.as_str())
        .bind(sample.node_count as i64)
        .bind(sample.sampled_at.timestamp())
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_usage_samples(
        &self,
        org_id: Uuid,
        since: i64,
    ) -> Result<Vec<UsageSample>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT org_id, backend, kind, node_count, sampled_at FROM org_usage_ledger \
             WHERE org_id = ? AND sampled_at >= ? ORDER BY sampled_at",
        ))
        .bind(org_id)
        .bind(since)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_usage_sample).collect())
    }

    async fn upsert_org_resource_group(
        &self,
        group: OrgResourceGroup,
    ) -> Result<OrgResourceGroup, SendableError> {
        let now = Utc::now().timestamp();
        let conflict = self.dialect().on_conflict_update(
            "org_id, backend, kind",
            &["desired", "dedicated", "updated_at"],
        );
        sqlx::query(&self.render(&format!(
            "INSERT INTO org_resource_groups (org_id, backend, kind, desired, dedicated, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?) {conflict}",
        )))
        .bind(group.org_id)
        .bind(group.backend.as_str())
        .bind(group.kind.as_str())
        .bind(group.desired as i64)
        .bind(group.dedicated)
        .bind(now)
        .execute(self.pool())
        .await?;
        Ok(group)
    }

    async fn list_all_resource_groups(&self) -> Result<Vec<OrgResourceGroup>, SendableError> {
        let rows =
            sqlx::query(&self.render(
                "SELECT org_id, backend, kind, desired, dedicated FROM org_resource_groups",
            ))
            .fetch_all(self.pool())
            .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_org_resource_group)
            .collect())
    }
}
