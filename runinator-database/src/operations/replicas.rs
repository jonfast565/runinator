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
    async fn agent_directive_queue_snapshot(
        &self,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
    ) -> Result<QueueSnapshot, SendableError> {
        let row = sqlx::query(&self.render(
            "SELECT COUNT(*) AS depth,
                    COALESCE(SUM(CASE WHEN claimed_at > ? THEN 1 ELSE 0 END), 0) AS claimed,
                    MIN(issued_at) AS oldest
             FROM agent_directives
             WHERE state IN ('pending', 'published', 'accepted') AND expires_at > ?",
        ))
        .bind(stale_before.timestamp())
        .bind(now.timestamp())
        .fetch_one(self.pool())
        .await?;
        let oldest: Option<i64> = row.try_get("oldest")?;
        Ok(QueueSnapshot {
            depth: row.try_get::<i64, _>("depth")?.max(0) as u64,
            claimed: row.try_get::<i64, _>("claimed")?.max(0) as u64,
            oldest_enqueued_at: oldest.and_then(|value| DateTime::from_timestamp(value, 0)),
        })
    }

    async fn enqueue_agent_directive(
        &self,
        replica_id: Uuid,
        kind: AgentDirectiveKind,
        expires_at: DateTime<Utc>,
    ) -> Result<AgentDirectiveRecord, SendableError> {
        let directive_id = Uuid::now_v7();
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "INSERT INTO agent_directives (directive_id, replica_id, kind_json, state, issued_at, expires_at, payload_json, attempts)
             VALUES (?, ?, ?, 'pending', ?, ?, 'null', 0)",
        ))
        .bind(directive_id)
        .bind(replica_id)
        .bind(serde_json::to_string(&kind)?)
        .bind(now)
        .bind(expires_at.timestamp())
        .execute(self.pool())
        .await?;
        self.fetch_agent_directive(directive_id)
            .await?
            .ok_or_else(|| {
                Box::new(std::io::Error::other(
                    "inserted agent directive disappeared",
                )) as SendableError
            })
    }

    async fn claim_due_agent_directives(
        &self,
        runtime_id: String,
        now: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<AgentDirectiveRecord>, SendableError> {
        let claimable = "state IN ('pending', 'published', 'accepted') AND expires_at > ? AND (claimed_at IS NULL OR claimed_at <= ?)";
        if self.dialect() == SqlDialect::MariaDb {
            sqlx::query(&self.render(&format!(
                "UPDATE agent_directives SET claimed_at = ?, claimed_by_runtime_id = ?, attempts = attempts + 1
                 WHERE directive_id IN (SELECT directive_id FROM (SELECT directive_id FROM agent_directives
                 WHERE {claimable} ORDER BY issued_at ASC LIMIT ?) AS claimable)"
            )))
            .bind(now.timestamp())
            .bind(runtime_id.as_str())
            .bind(now.timestamp())
            .bind(stale_before.timestamp())
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {AGENT_DIRECTIVE_COLUMNS} FROM agent_directives WHERE claimed_at = ? AND claimed_by_runtime_id = ? ORDER BY issued_at ASC"
            )))
            .bind(now.timestamp())
            .bind(runtime_id)
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(mappers::row_to_agent_directive).collect();
        }
        let rows = sqlx::query(&self.render(&format!(
            "UPDATE agent_directives SET claimed_at = ?, claimed_by_runtime_id = ?, attempts = attempts + 1
             WHERE directive_id IN (SELECT directive_id FROM agent_directives WHERE {claimable}
             ORDER BY issued_at ASC LIMIT ?{skip}) RETURNING {AGENT_DIRECTIVE_COLUMNS}",
            skip = self.dialect().skip_locked(),
        )))
        .bind(now.timestamp())
        .bind(runtime_id.as_str())
        .bind(now.timestamp())
        .bind(stale_before.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_agent_directive).collect()
    }

    async fn mark_agent_directive_published(
        &self,
        directive_id: Uuid,
    ) -> Result<(), SendableError> {
        let now = Utc::now().timestamp();
        sqlx::query(&self.render(
            "UPDATE agent_directives SET state = CASE WHEN state = 'accepted' THEN state ELSE 'published' END,
             published_at = COALESCE(published_at, ?), claimed_at = ?, message = NULL WHERE directive_id = ? AND completed_at IS NULL",
        ))
        .bind(now)
        .bind(now)
        .bind(directive_id)
        .execute(self.pool())
        .await?;
        Ok(())
    }

    async fn complete_agent_directive(
        &self,
        result: AgentDirectiveResult,
    ) -> Result<Option<AgentDirectiveRecord>, SendableError> {
        let now = Utc::now().timestamp();
        let (state, terminal) = match result.status {
            AgentDirectiveStatus::Accepted => ("accepted", false),
            AgentDirectiveStatus::Completed => ("completed", true),
            AgentDirectiveStatus::Failed => ("failed", true),
            AgentDirectiveStatus::Unsupported => ("unsupported", true),
        };
        sqlx::query(&self.render(
            "UPDATE agent_directives SET state = ?, payload_json = ?, message = ?, completed_at = ?, claimed_at = ?, claimed_by_runtime_id = NULL
             WHERE directive_id = ? AND state <> 'expired' AND completed_at IS NULL",
        ))
        .bind(state)
        .bind(result.payload.to_string())
        .bind(result.message)
        .bind(terminal.then_some(now))
        .bind(now)
        .bind(result.directive_id)
        .execute(self.pool())
        .await?;
        self.fetch_agent_directive(result.directive_id).await
    }

    async fn fetch_agent_directive(
        &self,
        directive_id: Uuid,
    ) -> Result<Option<AgentDirectiveRecord>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {AGENT_DIRECTIVE_COLUMNS} FROM agent_directives WHERE directive_id = ?"
        )))
        .bind(directive_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref()
            .map(mappers::row_to_agent_directive)
            .transpose()
    }

    async fn list_agent_directives(
        &self,
        replica_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AgentDirectiveRecord>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {AGENT_DIRECTIVE_COLUMNS} FROM agent_directives WHERE replica_id = ? ORDER BY issued_at DESC, directive_id DESC LIMIT ?"
        )))
        .bind(replica_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(self.pool())
        .await?;
        rows.iter().map(mappers::row_to_agent_directive).collect()
    }

    async fn expire_agent_directives(&self, now: DateTime<Utc>) -> Result<u64, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE agent_directives SET state = 'expired', completed_at = ?, claimed_at = NULL, claimed_by_runtime_id = NULL
             WHERE completed_at IS NULL AND expires_at <= ?",
        ))
        .bind(now.timestamp())
        .bind(now.timestamp())
        .execute(self.pool())
        .await?;
        Ok(result.affected())
    }

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
        let replica_id = request.replica_id.unwrap_or_else(Uuid::now_v7);
        if self.dialect() == SqlDialect::MariaDb {
            let conflict = SqlDialect::MariaDb.on_conflict_update(
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
        // Historical attribution, telemetry, and directives have independent retention windows.
        // Keep their replica owner until every referencing row has been archived or pruned rather
        // than nulling provenance or letting an ON DELETE cascade bypass those policies.
        let cutoff_ts = cutoff.timestamp();
        Ok(retry_delete(|| async {
            let mut tx = self.pool().begin().await?;

            let deleted = sqlx::query(&self.render(
                "DELETE FROM replicas WHERE last_heartbeat_at <= ?
                   AND NOT EXISTS (SELECT 1 FROM workflow_runs WHERE trigger_actor_replica_id = replicas.replica_id)
                   AND NOT EXISTS (SELECT 1 FROM workflow_effects WHERE current_executor_replica_id = replicas.replica_id OR last_executor_replica_id = replicas.replica_id)
                   AND NOT EXISTS (SELECT 1 FROM replica_samples WHERE replica_samples.replica_id = replicas.replica_id)
                   AND NOT EXISTS (SELECT 1 FROM agent_directives WHERE agent_directives.replica_id = replicas.replica_id)",
            ))
            .bind(cutoff_ts)
            .execute(&mut *tx)
            .await?
            .affected();

            tx.commit().await?;
            Ok(deleted)
        })
        .await?)
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

    async fn fetch_replica_by_runtime(
        &self,
        instance_id: String,
        runtime_id: String,
    ) -> Result<Option<ReplicaRecord>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {REPLICA_COLUMNS} FROM replicas WHERE instance_id = ? AND runtime_id = ?",
        )))
        .bind(instance_id)
        .bind(runtime_id)
        .fetch_optional(self.pool())
        .await?;
        row.as_ref().map(mappers::row_to_replica).transpose()
    }

    async fn fetch_replicas(
        &self,
        replica_type: Option<ReplicaKind>,
        status: Option<ReplicaStatus>,
        stale_before: DateTime<Utc>,
    ) -> Result<Vec<ReplicaRecord>, SendableError> {
        let rows = if let Some(replica_type) = replica_type {
            sqlx::query(&self.render("SELECT replica_id, replica_type, instance_id, runtime_id,
                        CASE
                            WHEN status = 'offline' THEN 'offline'
                            WHEN last_heartbeat_at <= ? THEN 'stale'
                            ELSE 'live'
                        END AS status,
                        display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at,
                        registered_by_principal_id, registered_by_kind, registered_by_org_id
                 FROM replicas WHERE replica_type = ? ORDER BY replica_type, instance_id, replica_id"))
            .bind(stale_before.timestamp())
            .bind(replica_type.as_str())
            .fetch_all(self.pool())
            .await?
        } else {
            sqlx::query(&self.render(
                "SELECT replica_id, replica_type, instance_id, runtime_id,
                        CASE
                            WHEN status = 'offline' THEN 'offline'
                            WHEN last_heartbeat_at <= ? THEN 'stale'
                            ELSE 'live'
                        END AS status,
                        display_name, host, port, base_path, observed_ip, version, attributes, first_seen_at, last_heartbeat_at, last_seen_at, offline_at,
                        registered_by_principal_id, registered_by_kind, registered_by_org_id
                 FROM replicas ORDER BY replica_type, instance_id, replica_id",
            ))
            .bind(stale_before.timestamp())
            .fetch_all(self.pool())
            .await?
        };
        let mut replicas = rows
            .iter()
            .map(mappers::row_to_replica)
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(status) = status {
            replicas.retain(|replica| replica.status == status);
        }
        Ok(replicas)
    }

    async fn count_running_effects_by_executor(&self) -> Result<Vec<(Uuid, i64)>, SendableError> {
        // a held executor claim (current_executor_replica_id set) marks an effect that is actively
        // executing on that worker, so grouping the live claims yields the running-task count per
        // replica. the claim moved onto the effect with the vm cutover; node runs are gone.
        let rows = sqlx::query(&self.render(
            "SELECT current_executor_replica_id AS replica_id, COUNT(*) AS running_count
             FROM workflow_effects
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
        Ok(retry_delete(|| async {
            sqlx::query(&self.render("DELETE FROM replica_samples WHERE sampled_at < ?"))
                .bind(cutoff.timestamp())
                .execute(self.pool())
                .await
                .map(|result| result.affected())
        })
        .await?)
    }

    async fn upsert_replica_provider_registration(
        &self,
        replica_id: Uuid,
        request: ReplicaProviderRegistrationRequest,
    ) -> Result<ReplicaProviderRegistration, SendableError> {
        let now = Utc::now().timestamp();
        let provider_json = serde_json::to_string(&request.provider)?;
        if self.dialect() == SqlDialect::MariaDb {
            let conflict = SqlDialect::MariaDb.on_conflict_update(
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
