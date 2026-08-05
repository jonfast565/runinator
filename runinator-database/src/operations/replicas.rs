//! replica registration, heartbeats, and telemetry.
//!
//! the `ReplicaStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> ReplicaStore for SqlStore<B>
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
    async fn register_replica(
        &self,
        request: ReplicaRegistrationRequest,
        observed_ip: Option<String>,
        registered_by: &AuthContext,
    ) -> Result<ReplicaRecord, SendableError> {
        let now = Utc::now().timestamp();
        // only recorded on the initial insert below (deliberately absent from both conflict-update
        // clauses), so a later re-registration under a different identity can't reassign ownership.
        let registered_by_principal_id = registered_by.principal_id;
        let registered_by_kind = registered_by.kind.as_str();
        let registered_by_org_id = registered_by.org_id;
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE replicas SET status = 'stale' WHERE instance_id = ? AND runtime_id <> ? AND status = 'live'",
                ))
                .bind(request.instance_id.as_str())
                .bind(request.runtime_id.as_str()),
            )
            .await?;
        let replica_id = Uuid::now_v7();
        if self.dialect() == SqlDialect::MySql {
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "instance_id, runtime_id",
                &[
                    "replica_type",
                    "status",
                    "display_name",
                    "host",
                    "port",
                    "base_path",
                    "observed_ip",
                    "version",
                    "attributes",
                    "last_heartbeat_at",
                    "last_seen_at",
                    "offline_at",
                ],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO replicas (replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at, registered_by_principal_id, registered_by_kind, registered_by_org_id)
                 VALUES (?, ?, ?, ?, 'live', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?) {conflict}",
            )))
            .bind(replica_id)
            .bind(request.replica_type.as_str())
            .bind(request.instance_id.as_str())
            .bind(request.runtime_id.as_str())
            .bind(request.display_name.clone())
            .bind(request.host.clone())
            .bind(request.port.map(i64::from))
            .bind(request.base_path.clone())
            .bind(observed_ip.clone())
            .bind(request.version.clone())
            .bind(request.attributes.to_string())
            .bind(now)
            .bind(now)
            .bind(now)
            .bind(registered_by_principal_id)
            .bind(registered_by_kind)
            .bind(registered_by_org_id)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {REPLICA_COLUMNS} FROM replicas WHERE instance_id = ? AND runtime_id = ?",
            )))
            .bind(request.instance_id)
            .bind(request.runtime_id)
            .fetch_one(self.pool())
            .await?;
            return mappers::row_to_replica(&row);
        }

        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO replicas (replica_id, replica_type, instance_id, runtime_id, status, display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at, registered_by_principal_id, registered_by_kind, registered_by_org_id)
             VALUES (?, ?, ?, ?, 'live', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, NULL, ?, ?, ?)
             ON CONFLICT(instance_id, runtime_id) DO UPDATE SET replica_type = excluded.replica_type, status = 'live', display_name = excluded.display_name, host = excluded.host, port = excluded.port, base_path = excluded.base_path, observed_ip = excluded.observed_ip, version = excluded.version, attributes = excluded.attributes, last_heartbeat_at = excluded.last_heartbeat_at, last_seen_at = excluded.last_seen_at, offline_at = NULL
             RETURNING {REPLICA_COLUMNS}",
        )))
        .bind(replica_id)
        .bind(request.replica_type.as_str())
        .bind(request.instance_id)
        .bind(request.runtime_id)
        .bind(request.display_name)
        .bind(request.host)
        .bind(request.port.map(i64::from))
        .bind(request.base_path)
        .bind(observed_ip)
        .bind(request.version)
        .bind(request.attributes.to_string())
        .bind(now)
        .bind(now)
        .bind(now)
        .bind(registered_by_principal_id)
        .bind(registered_by_kind)
        .bind(registered_by_org_id)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_replica(&row)
    }

    async fn heartbeat_replica(
        &self,
        replica_id: Uuid,
        request: ReplicaHeartbeatRequest,
        observed_ip: Option<String>,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        let now = Utc::now().timestamp();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE replicas SET status = 'live', display_name = COALESCE(?, display_name), host = COALESCE(?, host), port = COALESCE(?, port), base_path = COALESCE(?, base_path), observed_ip = COALESCE(?, observed_ip), attributes = COALESCE(?, attributes), last_heartbeat_at = ?, last_seen_at = ?, offline_at = NULL
                     WHERE replica_id = ? AND runtime_id = ?",
                ))
                .bind(request.display_name.clone())
                .bind(request.host.clone())
                .bind(request.port.map(i64::from))
                .bind(request.base_path.clone())
                .bind(observed_ip.clone())
                .bind(Some(request.attributes.to_string()))
                .bind(now)
                .bind(now)
                .bind(replica_id)
                .bind(request.runtime_id.as_str()),
            )
            .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_COLUMNS} FROM replicas WHERE replica_id = ? AND runtime_id = ?",
        )))
        .bind(replica_id)
        .bind(request.runtime_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_replica).transpose()
    }

    async fn mark_replica_offline(
        &self,
        replica_id: Uuid,
        runtime_id: String,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        let now = Utc::now().timestamp();
        self.pool()
            .execute(
                sqlx::query(&self.render(
                    "UPDATE replicas SET status = 'offline', offline_at = ?, last_seen_at = ? WHERE replica_id = ? AND runtime_id = ?",
                ))
                .bind(now)
                .bind(now)
                .bind(replica_id)
                .bind(runtime_id.as_str()),
            )
            .await?;
        let row = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_COLUMNS} FROM replicas WHERE replica_id = ? AND runtime_id = ?",
        )))
        .bind(replica_id)
        .bind(runtime_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_replica).transpose()
    }

    async fn reap_inactive_replicas(&self, cutoff: DateTime<Utc>) -> Result<u64, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE replicas SET status = 'offline', offline_at = ? WHERE last_heartbeat_at <= ? AND status <> 'offline'",
        ))
        .bind(now)
        .bind(cutoff.timestamp())
        .execute(self.pool())
        .await?;
        Ok(result.affected())
    }

    async fn delete_expired_replicas(&self, cutoff: DateTime<Utc>) -> Result<u64, SendableError> {
        // null the historical attribution pointers (restrict-mode foreign keys) before deleting so
        // the delete does not error; provider registrations cascade. a replica still claimed as a node
        // run's current executor is excluded from the delete and left until that run resolves.
        let cutoff_ts = cutoff.timestamp();
        let mut tx = self.pool().begin().await?;

        sqlx::query(&self.render(
            "UPDATE workflow_runs SET trigger_actor_replica_id = NULL
             WHERE trigger_actor_replica_id IN
                 (SELECT replica_id FROM replicas WHERE last_heartbeat_at <= ?)",
        ))
        .bind(cutoff_ts)
        .execute(&mut *tx)
        .await?;

        sqlx::query(&self.render(
            "UPDATE workflow_node_runs SET last_executor_replica_id = NULL
             WHERE last_executor_replica_id IN
                 (SELECT replica_id FROM replicas WHERE last_heartbeat_at <= ?)",
        ))
        .bind(cutoff_ts)
        .execute(&mut *tx)
        .await?;

        let deleted = sqlx::query(&self.render(
            "DELETE FROM replicas WHERE last_heartbeat_at <= ? AND replica_id NOT IN
                 (SELECT current_executor_replica_id FROM workflow_node_runs
                  WHERE current_executor_replica_id IS NOT NULL)",
        ))
        .bind(cutoff_ts)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(deleted.affected())
    }

    async fn fetch_replica(
        &self,
        replica_id: Uuid,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_COLUMNS} FROM replicas WHERE replica_id = ?",
        )))
        .bind(replica_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_replica).transpose()
    }

    async fn count_running_node_runs_by_executor(&self) -> Result<Vec<(Uuid, i64)>, SendableError> {
        // a held executor claim (current_executor_replica_id set) marks a node run that is actively
        // executing on that worker, so grouping the live claims yields the running-task count per
        // replica.
        let rows = sqlx::query(&self.render(
            "SELECT current_executor_replica_id AS replica_id, COUNT(*) AS running_count
             FROM workflow_node_runs
             WHERE current_executor_replica_id IS NOT NULL
             GROUP BY current_executor_replica_id",
        ))
        .fetch_all(self.pool())
        .await?;
        let mut counts = Vec::with_capacity(rows.len());
        for row in &rows {
            let replica_id: Uuid = row.try_get("replica_id")?;
            let running_count: i64 = row.try_get("running_count")?;
            counts.push((replica_id, running_count));
        }
        Ok(counts)
    }

    async fn insert_replica_sample(&self, sample: ReplicaSample) -> Result<(), SendableError> {
        let data = serde_json::to_string(&sample)?;
        sqlx::query(&self.render(
            "INSERT INTO replica_samples (id, replica_id, sampled_at, data) VALUES (?, ?, ?, ?)",
        ))
        .bind(Uuid::now_v7())
        .bind(sample.replica_id)
        .bind(sample.sampled_at.timestamp())
        .bind(data)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn fetch_replica_samples(
        &self,
        replica_id: Uuid,
        since: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ReplicaSample>, SendableError> {
        // newest first with a bound, then reverse to oldest-first so charts read left-to-right.
        let rows = sqlx::query(&self.render(
            "SELECT replica_id, sampled_at, data FROM replica_samples
             WHERE replica_id = ? AND sampled_at >= ? ORDER BY sampled_at DESC, id DESC LIMIT ?",
        ))
        .bind(replica_id)
        .bind(since.timestamp())
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        let mut samples = rows
            .iter()
            .map(mappers::row_to_replica_sample)
            .collect::<Vec<_>>();
        samples.reverse();
        Ok(samples)
    }

    async fn prune_replica_samples(&self, cutoff: DateTime<Utc>) -> Result<u64, SendableError> {
        let result = sqlx::query(&self.render("DELETE FROM replica_samples WHERE sampled_at < ?"))
            .bind(cutoff.timestamp())
            .execute(self.pool())
            .await?;
        Ok(result.affected())
    }

    async fn upsert_replica_provider_registration(
        &self,
        replica_id: Uuid,
        request: ReplicaProviderRegistrationRequest,
    ) -> Result<ReplicaProviderRegistration, SendableError> {
        let now = Utc::now().timestamp();
        let provider_json = serde_json::to_string(&request.provider)?;
        if self.dialect() == SqlDialect::MySql {
            let conflict = queries::on_conflict_update(
                SqlDialect::MySql,
                "replica_id, provider_name",
                &["provider_json", "last_registered_at", "last_heartbeat_at"],
            );
            sqlx::query(&self.render(&format!(
                "INSERT INTO replica_provider_registrations (replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at)
                 VALUES (?, ?, ?, ?, ?, ?) {conflict}",
            )))
            .bind(replica_id)
            .bind(request.provider.name.as_str())
            .bind(provider_json)
            .bind(now)
            .bind(now)
            .bind(now)
            .execute(self.pool())
            .await?;
            let row = sqlx::query(&self.render(&format!(
                "SELECT {REPLICA_PROVIDER_COLUMNS} FROM replica_provider_registrations WHERE replica_id = ? AND provider_name = ?",
            )))
            .bind(replica_id)
            .bind(request.provider.name.as_str())
            .fetch_one(self.pool())
            .await?;
            return mappers::row_to_replica_provider_registration(&row);
        }
        let row = sqlx::query(&self.render(&format!(
            "INSERT INTO replica_provider_registrations (replica_id, provider_name, provider_json, first_registered_at, last_registered_at, last_heartbeat_at)
             VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT(replica_id, provider_name) DO UPDATE SET provider_json = excluded.provider_json, last_registered_at = excluded.last_registered_at, last_heartbeat_at = excluded.last_heartbeat_at
             RETURNING {REPLICA_PROVIDER_COLUMNS}",
        )))
        .bind(replica_id)
        .bind(request.provider.name.as_str())
        .bind(provider_json)
        .bind(now)
        .bind(now)
        .bind(now)
        .fetch_one(self.pool())
        .await?;
        mappers::row_to_replica_provider_registration(&row)
    }

    async fn fetch_replica_provider_registrations(
        &self,
        replica_id: Uuid,
    ) -> Result<Vec<ReplicaProviderRegistration>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_PROVIDER_COLUMNS} FROM replica_provider_registrations WHERE replica_id = ? ORDER BY provider_name"
        )))
        .bind(replica_id)
        .fetch_all(self.pool())
        .await?;
        rows.iter()
            .map(mappers::row_to_replica_provider_registration)
            .collect()
    }
}
