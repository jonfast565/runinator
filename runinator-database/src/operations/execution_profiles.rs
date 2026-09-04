use super::*;

const PROFILE_COLUMNS: &str = "id, org_id, name, description, credential_scopes, collection_json, exposure_json, config_version, config_digest, enabled, current_revision, current_digest, current_publisher_id, published_at, expires_at, refresh_requested_at, health, last_error, created_at, updated_at";
const REVISION_COLUMNS: &str =
    "profile_id, revision, digest, size_bytes, publisher_id, expires_at, created_at, uri";
const AGENT_STATUS_COLUMNS: &str = "profile_id, agent_id, config_digest, approval, last_seen_at, last_attempt_at, last_success_at, last_error";
const OPERATION_COLUMNS: &str = "id, profile_id, config_digest, kind, state, requested_at, requested_by, claimed_by, started_at, lease_expires_at, completed_at, error";

impl<B> ExecutionProfileStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn upsert_execution_profile(
        &self,
        profile: &ExecutionProfile,
    ) -> Result<ExecutionProfile, SendableError> {
        let existing = self.fetch_execution_profile(profile.id).await?;
        if existing.is_some() {
            sqlx::query(&self.render("UPDATE execution_profiles SET name = ?, description = ?, credential_scopes = ?, collection_json = ?, exposure_json = ?, config_version = ?, config_digest = ?, enabled = ?, current_revision = ?, current_digest = ?, current_publisher_id = ?, published_at = ?, expires_at = ?, refresh_requested_at = ?, health = ?, last_error = ?, updated_at = ? WHERE id = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))"))
                .bind(&profile.name).bind(&profile.description)
                .bind(serde_json::to_string(&profile.credential_scopes)?)
                .bind(serde_json::to_string(&profile.collection)?)
                .bind(serde_json::to_string(&profile.exposure)?)
                .bind(profile.config_version).bind(&profile.config_digest).bind(profile.enabled)
                .bind(profile.current_revision).bind(profile.current_digest.clone())
                .bind(profile.current_publisher_id)
                .bind(profile.published_at.map(|value| value.timestamp()))
                .bind(profile.expires_at.map(|value| value.timestamp()))
                .bind(profile.refresh_requested_at.map(|value| value.timestamp()))
                .bind(profile.health.as_str()).bind(profile.last_error.clone()).bind(profile.updated_at.timestamp()).bind(profile.id)
                .bind(profile.org_id).bind(profile.org_id).execute(self.pool()).await?;
        } else {
            sqlx::query(&self.render(&format!("INSERT INTO execution_profiles ({PROFILE_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")))
                .bind(profile.id).bind(profile.org_id).bind(&profile.name).bind(&profile.description)
                .bind(serde_json::to_string(&profile.credential_scopes)?)
                .bind(serde_json::to_string(&profile.collection)?)
                .bind(serde_json::to_string(&profile.exposure)?)
                .bind(profile.config_version).bind(&profile.config_digest).bind(profile.enabled)
                .bind(profile.current_revision).bind(profile.current_digest.clone())
                .bind(profile.current_publisher_id)
                .bind(profile.published_at.map(|v| v.timestamp())).bind(profile.expires_at.map(|v| v.timestamp()))
                .bind(profile.refresh_requested_at.map(|v| v.timestamp()))
                .bind(profile.health.as_str()).bind(profile.last_error.clone())
                .bind(profile.created_at.timestamp()).bind(profile.updated_at.timestamp())
                .execute(self.pool()).await?;
        }
        self.fetch_execution_profile(profile.id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "execution profile disappeared after upsert",
                )) as SendableError
            })
    }

    async fn list_execution_profiles(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<ExecutionProfile>, SendableError> {
        let rows = sqlx::query(&self.render(&format!("SELECT {PROFILE_COLUMNS} FROM execution_profiles WHERE (org_id = ? OR (org_id IS NULL AND ? IS NULL)) ORDER BY name")))
            .bind(org_id).bind(org_id).fetch_all(self.pool()).await?;
        Ok(rows.iter().map(mappers::row_to_execution_profile).collect())
    }

    async fn fetch_execution_profile(
        &self,
        id: Uuid,
    ) -> Result<Option<ExecutionProfile>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {PROFILE_COLUMNS} FROM execution_profiles WHERE id = ?"
        )))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_execution_profile))
    }

    async fn fetch_execution_profile_by_name(
        &self,
        org_id: Option<Uuid>,
        name: &str,
    ) -> Result<Option<ExecutionProfile>, SendableError> {
        let row = sqlx::query(&self.render(&format!("SELECT {PROFILE_COLUMNS} FROM execution_profiles WHERE name = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))")))
            .bind(name).bind(org_id).bind(org_id).fetch_optional(self.pool()).await?;
        Ok(row.as_ref().map(mappers::row_to_execution_profile))
    }

    async fn insert_execution_profile_revision(
        &self,
        revision: &ExecutionProfileRevision,
    ) -> Result<ExecutionProfileRevision, SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render(&format!("INSERT INTO execution_profile_revisions ({REVISION_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")))
            .bind(revision.profile_id).bind(revision.revision).bind(&revision.digest).bind(revision.size_bytes)
            .bind(revision.publisher_id).bind(revision.expires_at.map(|v| v.timestamp()))
            .bind(revision.created_at.timestamp()).bind(&revision.uri).execute(&mut *tx).await?;
        sqlx::query(&self.render("UPDATE execution_profiles SET current_revision = ?, current_digest = ?, current_publisher_id = ?, published_at = ?, expires_at = ?, health = 'ready', last_error = NULL, updated_at = ? WHERE id = ?"))
            .bind(revision.revision).bind(&revision.digest).bind(revision.publisher_id).bind(revision.created_at.timestamp())
            .bind(revision.expires_at.map(|v| v.timestamp())).bind(revision.created_at.timestamp())
            .bind(revision.profile_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(revision.clone())
    }

    async fn fetch_execution_profile_revision(
        &self,
        profile_id: Uuid,
        revision: i64,
    ) -> Result<Option<ExecutionProfileRevision>, SendableError> {
        let row = sqlx::query(&self.render(&format!("SELECT {REVISION_COLUMNS} FROM execution_profile_revisions WHERE profile_id = ? AND revision = ?")))
            .bind(profile_id).bind(revision).fetch_optional(self.pool()).await?;
        Ok(row.as_ref().map(mappers::row_to_execution_profile_revision))
    }

    async fn delete_execution_profile(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
    ) -> Result<bool, SendableError> {
        let mut tx = self.pool().begin().await?;
        for table in ["resource_grants", "resource_ownership"] {
            sqlx::query(&self.render(&format!(
                "DELETE FROM {table} WHERE resource_type = 'execution_profile' AND resource_id IN (SELECT id FROM execution_profiles WHERE id = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL)))"
            )))
            .bind(id).bind(org_id).bind(org_id).execute(&mut *tx).await?;
        }
        let result = sqlx::query(&self.render("DELETE FROM execution_profiles WHERE id = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))"))
            .bind(id).bind(org_id).bind(org_id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(result.affected() == 1)
    }

    async fn request_execution_profile_refresh(
        &self,
        id: Uuid,
        org_id: Option<Uuid>,
        requested_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render("UPDATE execution_profiles SET refresh_requested_at = ?, health = 'unpublished', last_error = NULL, updated_at = ? WHERE id = ? AND enabled = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))"))
            .bind(requested_at.timestamp()).bind(requested_at.timestamp()).bind(id).bind(true)
            .bind(org_id).bind(org_id).execute(self.pool()).await?;
        Ok(result.affected() == 1)
    }

    async fn update_execution_profile_health(
        &self,
        id: Uuid,
        health: runinator_models::execution_profiles::ExecutionProfileHealth,
        error: Option<String>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE execution_profiles SET health = ?, last_error = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(health.as_str())
        .bind(error)
        .bind(chrono::Utc::now().timestamp())
        .bind(id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() == 1)
    }

    async fn upsert_execution_profile_agent_status(
        &self,
        status: &runinator_models::execution_profiles::ExecutionProfileAgentStatus,
    ) -> Result<(), SendableError> {
        let updated = sqlx::query(&self.render(
            "UPDATE execution_profile_agent_statuses SET approval = ?, last_seen_at = ?, last_attempt_at = COALESCE(?, last_attempt_at), last_success_at = COALESCE(?, last_success_at), last_error = CASE WHEN ? IS NULL THEN last_error ELSE ? END WHERE profile_id = ? AND agent_id = ? AND config_digest = ?",
        ))
        .bind(status.approval.as_str())
        .bind(status.last_seen_at.timestamp())
        .bind(status.last_attempt_at.map(|value| value.timestamp()))
        .bind(status.last_success_at.map(|value| value.timestamp()))
        .bind(status.last_attempt_at.map(|value| value.timestamp()))
        .bind(status.last_error.clone())
        .bind(status.profile_id)
        .bind(status.agent_id)
        .bind(&status.config_digest)
        .execute(self.pool())
        .await?;
        if updated.affected() == 1 {
            return Ok(());
        }

        let inserted = sqlx::query(&self.render(&format!(
            "INSERT INTO execution_profile_agent_statuses ({AGENT_STATUS_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(status.profile_id)
        .bind(status.agent_id)
        .bind(&status.config_digest)
        .bind(status.approval.as_str())
        .bind(status.last_seen_at.timestamp())
        .bind(status.last_attempt_at.map(|value| value.timestamp()))
        .bind(status.last_success_at.map(|value| value.timestamp()))
        .bind(status.last_error.clone())
        .execute(self.pool())
        .await;
        match inserted {
            Ok(_) => Ok(()),
            Err(error) if is_unique_violation(&error) => {
                sqlx::query(&self.render(
                    "UPDATE execution_profile_agent_statuses SET approval = ?, last_seen_at = ?, last_attempt_at = COALESCE(?, last_attempt_at), last_success_at = COALESCE(?, last_success_at), last_error = CASE WHEN ? IS NULL THEN last_error ELSE ? END WHERE profile_id = ? AND agent_id = ? AND config_digest = ?",
                ))
                .bind(status.approval.as_str())
                .bind(status.last_seen_at.timestamp())
                .bind(status.last_attempt_at.map(|value| value.timestamp()))
                .bind(status.last_success_at.map(|value| value.timestamp()))
                .bind(status.last_attempt_at.map(|value| value.timestamp()))
                .bind(status.last_error.clone())
                .bind(status.profile_id)
                .bind(status.agent_id)
                .bind(&status.config_digest)
                .execute(self.pool())
                .await?;
                Ok(())
            }
            Err(error) => Err(Box::new(error)),
        }
    }

    async fn list_execution_profile_agent_statuses(
        &self,
        profile_id: Uuid,
        config_digest: &str,
    ) -> Result<Vec<runinator_models::execution_profiles::ExecutionProfileAgentStatus>, SendableError>
    {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {AGENT_STATUS_COLUMNS} FROM execution_profile_agent_statuses WHERE profile_id = ? AND config_digest = ? ORDER BY last_seen_at DESC"
        )))
        .bind(profile_id)
        .bind(config_digest)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_execution_profile_agent_status)
            .collect())
    }

    async fn insert_execution_profile_operation(
        &self,
        operation: &runinator_models::execution_profiles::ExecutionProfileOperation,
    ) -> Result<runinator_models::execution_profiles::ExecutionProfileOperation, SendableError>
    {
        sqlx::query(&self.render(&format!(
            "INSERT INTO execution_profile_operations ({OPERATION_COLUMNS}, active_key) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(operation.id)
        .bind(operation.profile_id)
        .bind(&operation.config_digest)
        .bind(operation.kind.as_str())
        .bind(operation.state.as_str())
        .bind(operation.requested_at.timestamp())
        .bind(operation.requested_by)
        .bind(operation.claimed_by)
        .bind(operation.started_at.map(|value| value.timestamp()))
        .bind(operation.lease_expires_at.map(|value| value.timestamp()))
        .bind(operation.completed_at.map(|value| value.timestamp()))
        .bind(operation.error.clone())
        .bind(operation.state.is_active().then_some(1_i64))
        .execute(self.pool())
        .await?;
        Ok(operation.clone())
    }

    async fn fetch_latest_execution_profile_operation(
        &self,
        profile_id: Uuid,
        config_digest: &str,
    ) -> Result<
        Option<runinator_models::execution_profiles::ExecutionProfileOperation>,
        SendableError,
    > {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {OPERATION_COLUMNS} FROM execution_profile_operations WHERE profile_id = ? AND config_digest = ? ORDER BY requested_at DESC LIMIT 1"
        )))
        .bind(profile_id)
        .bind(config_digest)
        .fetch_optional(self.pool())
        .await?;
        Ok(row
            .as_ref()
            .map(mappers::row_to_execution_profile_operation))
    }

    async fn list_pending_execution_profile_operations(
        &self,
        org_id: Option<Uuid>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<Vec<runinator_models::execution_profiles::ExecutionProfileOperation>, SendableError>
    {
        let rows = sqlx::query(&self.render(
            "SELECT operation.id, operation.profile_id, operation.config_digest, operation.kind, operation.state, operation.requested_at, operation.requested_by, operation.claimed_by, operation.started_at, operation.lease_expires_at, operation.completed_at, operation.error FROM execution_profile_operations operation INNER JOIN execution_profiles profile ON profile.id = operation.profile_id WHERE (operation.state = 'queued' OR (operation.state = 'running' AND operation.lease_expires_at <= ?)) AND operation.config_digest = profile.config_digest AND (profile.org_id = ? OR (profile.org_id IS NULL AND ? IS NULL)) ORDER BY operation.requested_at ASC",
        ))
        .bind(now.timestamp())
        .bind(org_id)
        .bind(org_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows
            .iter()
            .map(mappers::row_to_execution_profile_operation)
            .collect())
    }

    async fn claim_execution_profile_operation(
        &self,
        operation_id: Uuid,
        agent_id: Uuid,
        config_digest: &str,
        started_at: chrono::DateTime<chrono::Utc>,
        lease_expires_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<
        Option<runinator_models::execution_profiles::ExecutionProfileOperation>,
        SendableError,
    > {
        let result = sqlx::query(&self.render(
            "UPDATE execution_profile_operations SET state = 'running', claimed_by = ?, started_at = ?, lease_expires_at = ? WHERE id = ? AND config_digest = ? AND (state = 'queued' OR (state = 'running' AND lease_expires_at <= ?))",
        ))
        .bind(agent_id)
        .bind(started_at.timestamp())
        .bind(lease_expires_at.timestamp())
        .bind(operation_id)
        .bind(config_digest)
        .bind(started_at.timestamp())
        .execute(self.pool())
        .await?;
        if result.affected() != 1 {
            return Ok(None);
        }
        let row = sqlx::query(&self.render(&format!(
            "SELECT {OPERATION_COLUMNS} FROM execution_profile_operations WHERE id = ?"
        )))
        .bind(operation_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row
            .as_ref()
            .map(mappers::row_to_execution_profile_operation))
    }

    async fn complete_execution_profile_operation(
        &self,
        operation_id: Uuid,
        agent_id: Uuid,
        state: runinator_models::execution_profiles::ExecutionProfileOperationState,
        error: Option<String>,
        completed_at: chrono::DateTime<chrono::Utc>,
    ) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE execution_profile_operations SET state = ?, active_key = NULL, error = ?, completed_at = ?, lease_expires_at = NULL WHERE id = ? AND claimed_by = ? AND state = 'running'",
        ))
        .bind(state.as_str())
        .bind(error)
        .bind(completed_at.timestamp())
        .bind(operation_id)
        .bind(agent_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() == 1)
    }
}
