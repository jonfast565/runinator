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
    async fn create_org(&self, name: String, slug: String) -> Result<Organization, SendableError> {
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
        sqlx::query(&self.render(
            "DELETE FROM role_assignments WHERE principal_kind = 'user' AND principal_id = ? AND scope_key = ?",
        )).bind(user_id).bind(format!("organization:{org_id}"))
            .execute(self.pool()).await?;
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
