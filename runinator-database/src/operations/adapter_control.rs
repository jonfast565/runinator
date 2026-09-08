//! Adapter delivery journals, publication recovery, and operator gates.
use super::*;
use runinator_models::adapter_control::*;
use runinator_models::ingress_control::{ExternalIngressGateMode, ExternalIngressRecord};
use runinator_store::roles::{
    AdapterControlStore, AdapterPollDispatch, IngressStore, OrchestrationStore,
};

fn delivery(data: String, state: String) -> Result<AdapterDeliveryRecord, SendableError> {
    let mut record: AdapterDeliveryRecord = serde_json::from_str(&data)?;
    record.state = state;
    Ok(record)
}

impl<B> AdapterControlStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> bool: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<Uuid>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
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
    async fn record_adapter_delivery(
        &self,
        record: AdapterDeliveryRecord,
    ) -> Result<AdapterDeliveryRecord, SendableError> {
        let key = record
            .event
            .as_ref()
            .map(|event| format!("{}:{}", record.origin.revision, event.delivery_id))
            .unwrap_or_else(|| record.id.to_string());
        if let Some(row) = sqlx::query(&self.render(
            "SELECT data, state FROM adapter_deliveries WHERE adapter_id = ? AND delivery_key = ?",
        ))
        .bind(record.origin.adapter_id)
        .bind(&key)
        .fetch_optional(self.pool())
        .await?
        {
            return delivery(row.get("data"), row.get("state"));
        }
        let count = sqlx::query(&self.render("SELECT COUNT(*) AS count FROM adapter_deliveries WHERE adapter_id = ? AND state NOT IN ('applied', 'rejected', 'dropped')"))
            .bind(record.origin.adapter_id).fetch_one(self.pool()).await?.get::<i64,_>("count");
        if count >= 10000 {
            return Err(crate::errors::ADAPTER_QUEUE_FULL.error("adapter delivery journal is full"));
        }
        let result = sqlx::query(&self.render("INSERT INTO adapter_deliveries (id, adapter_id, delivery_key, data, state, received_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?)"))
            .bind(record.id).bind(record.origin.adapter_id).bind(&key).bind(serde_json::to_string(&record)?).bind(&record.state)
            .bind(record.received_at.timestamp()).bind(record.updated_at.timestamp()).execute(self.pool()).await;
        match result {
            Ok(_) => Ok(record),
            Err(error) if is_unique_violation(&error) => {
                let row = sqlx::query(&self.render("SELECT data, state FROM adapter_deliveries WHERE adapter_id = ? AND delivery_key = ?"))
                    .bind(record.origin.adapter_id).bind(key).fetch_one(self.pool()).await?;
                delivery(row.get("data"), row.get("state"))
            }
            Err(error) => Err(Box::new(error)),
        }
    }
    async fn fetch_adapter_deliveries(
        &self,
        adapter_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AdapterDeliveryRecord>, SendableError> {
        sqlx::query(&self.render("SELECT data, state FROM adapter_deliveries WHERE adapter_id = ? ORDER BY received_at DESC, id DESC LIMIT ?"))
            .bind(adapter_id).bind(limit.clamp(1,500)).fetch_all(self.pool()).await?.into_iter().map(|row| delivery(row.get("data"),row.get("state"))).collect()
    }
    async fn fetch_adapter_delivery(
        &self,
        id: Uuid,
    ) -> Result<Option<AdapterDeliveryRecord>, SendableError> {
        sqlx::query(&self.render("SELECT data, state FROM adapter_deliveries WHERE id = ?"))
            .bind(id)
            .fetch_optional(self.pool())
            .await?
            .map(|row| delivery(row.get("data"), row.get("state")))
            .transpose()
    }
    async fn update_adapter_delivery(
        &self,
        record: AdapterDeliveryRecord,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render(
            "UPDATE adapter_deliveries SET data = ?, state = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(serde_json::to_string(&record)?)
        .bind(record.state)
        .bind(record.updated_at.timestamp())
        .bind(record.id)
        .execute(self.pool())
        .await?;
        Ok(())
    }
    async fn decide_adapter_delivery(
        &self,
        id: Uuid,
        approve: bool,
    ) -> Result<bool, SendableError> {
        let Some(mut record) = self.fetch_adapter_delivery(id).await? else {
            return Ok(false);
        };
        record.approved = approve;
        record.error = None;
        Ok(sqlx::query(&self.render("UPDATE adapter_deliveries SET state = ?, data = ?, updated_at = ? WHERE id = ? AND state IN ('held', 'failed', 'rejected')"))
            .bind(if approve {"approved"} else {"dropped"}).bind(serde_json::to_string(&record)?).bind(Utc::now().timestamp()).bind(id).execute(self.pool()).await?.affected()>0)
    }
    async fn claim_adapter_delivery(
        &self,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<AdapterDeliveryRecord>, SendableError> {
        let row = sqlx::query(&self.render("SELECT id, data, state FROM adapter_deliveries WHERE state IN ('pending', 'approved') OR (state = 'applying' AND claimed_until <= ?) ORDER BY received_at, id LIMIT 1"))
            .bind(now.timestamp()).fetch_optional(self.pool()).await?;
        let Some(row) = row else { return Ok(None) };
        let id: Uuid = row.get("id");
        let changed = sqlx::query(&self.render("UPDATE adapter_deliveries SET state = 'applying', claim_token = ?, claimed_until = ? WHERE id = ? AND (state IN ('pending', 'approved') OR (state = 'applying' AND claimed_until <= ?))"))
            .bind(token).bind(now.timestamp()+300).bind(id).bind(now.timestamp()).execute(self.pool()).await?.affected();
        if changed == 0 {
            return Ok(None);
        }
        delivery(row.get("data"), row.get("state")).map(Some)
    }
    async fn finish_adapter_delivery(
        &self,
        record: AdapterDeliveryRecord,
        token: Uuid,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render("UPDATE adapter_deliveries SET data = ?, state = ?, updated_at = ?, claim_token = NULL, claimed_until = NULL WHERE id = ? AND claim_token = ?"))
            .bind(serde_json::to_string(&record)?).bind(record.state).bind(record.updated_at.timestamp()).bind(record.id).bind(token).execute(self.pool()).await?.affected()>0)
    }
    async fn adapter_inspection(
        &self,
        adapter_id: Uuid,
    ) -> Result<AdapterInspection, SendableError> {
        let row =
            sqlx::query(&self.render("SELECT mode FROM adapter_inspection WHERE adapter_id = ?"))
                .bind(adapter_id)
                .fetch_optional(self.pool())
                .await?;
        let mode = match row.map(|row| row.get::<String, _>("mode")).as_deref() {
            Some("paused") => ExternalIngressGateMode::Paused,
            Some("review") => ExternalIngressGateMode::Review,
            _ => ExternalIngressGateMode::Disabled,
        };
        Ok(AdapterInspection { adapter_id, mode })
    }
    async fn set_adapter_inspection(
        &self,
        inspection: AdapterInspection,
    ) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("DELETE FROM adapter_inspection WHERE adapter_id = ?"))
            .bind(inspection.adapter_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            &self.render("INSERT INTO adapter_inspection (adapter_id, mode) VALUES (?, ?)"),
        )
        .bind(inspection.adapter_id)
        .bind(inspection.mode.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
    async fn approve_external_ingress(
        &self,
        id: Uuid,
        actor: Uuid,
        org_id: Option<Uuid>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render("UPDATE ingress_control_events SET state = 'approved', reviewed_by = ?, caller_org_id = ?, last_error = NULL WHERE id = ? AND state IN ('held', 'failed')"))
            .bind(actor).bind(org_id).bind(id).execute(self.pool()).await?.affected()>0)
    }
    async fn claim_approved_external_ingress(
        &self,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<ExternalIngressRecord>, SendableError> {
        let row=sqlx::query(&self.render("SELECT id FROM ingress_control_events WHERE state = 'approved' OR (state = 'applying' AND (claimed_until IS NULL OR claimed_until <= ?)) ORDER BY received_at, id LIMIT 1"))
            .bind(now.timestamp()).fetch_optional(self.pool()).await?;
        let Some(row) = row else { return Ok(None) };
        let id: Uuid = row.get("id");
        let changed=sqlx::query(&self.render("UPDATE ingress_control_events SET state = 'applying', claim_token = ?, claimed_until = ? WHERE id = ? AND (state = 'approved' OR (state = 'applying' AND (claimed_until IS NULL OR claimed_until <= ?)))"))
            .bind(token).bind(now.timestamp()+300).bind(id).bind(now.timestamp()).execute(self.pool()).await?.affected();
        if changed == 0 {
            return Ok(None);
        }
        self.fetch_external_ingress_record(id).await
    }
    async fn finish_approved_external_ingress(
        &self,
        id: Uuid,
        token: Uuid,
        error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render("UPDATE ingress_control_events SET state = ?, last_error = ?, resolved_at = ?, claim_token = NULL, claimed_until = NULL WHERE id = ? AND claim_token = ?"))
            .bind(if error.is_some(){"failed"}else{"applied"}).bind(error).bind(now.timestamp()).bind(id).bind(token).execute(self.pool()).await?.affected()>0)
    }
    async fn orchestration_debug_control(
        &self,
        pipeline_id: Uuid,
    ) -> Result<OrchestrationDebugControl, SendableError> {
        Ok(sqlx::query(&self.render(
            "SELECT paused, steps FROM orchestration_debug_controls WHERE pipeline_id = ?",
        ))
        .bind(pipeline_id)
        .fetch_optional(self.pool())
        .await?
        .map(|row| OrchestrationDebugControl {
            paused: row.get("paused"),
            steps: row.get("steps"),
        })
        .unwrap_or_default())
    }
    async fn set_orchestration_debug_control(
        &self,
        pipeline_id: Uuid,
        control: OrchestrationDebugControl,
    ) -> Result<(), SendableError> {
        let mut tx = self.pool().begin().await?;
        sqlx::query(&self.render("DELETE FROM orchestration_debug_controls WHERE pipeline_id = ?"))
            .bind(pipeline_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&self.render("INSERT INTO orchestration_debug_controls (pipeline_id, paused, steps) VALUES (?, ?, ?)"))
            .bind(pipeline_id).bind(control.paused).bind(control.steps.clamp(0,1)).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
    async fn take_orchestration_debug_permit(
        &self,
        pipeline_id: Uuid,
    ) -> Result<bool, SendableError> {
        if !self.orchestration_debug_control(pipeline_id).await?.paused {
            return Ok(true);
        }
        Ok(sqlx::query(&self.render("UPDATE orchestration_debug_controls SET steps = steps - 1 WHERE pipeline_id = ? AND steps > 0 AND paused = ?"))
            .bind(pipeline_id).bind(true).execute(self.pool()).await?.affected()>0)
    }
    async fn fetch_adapter_poll_attempts(
        &self,
        adapter_id: Uuid,
        limit: i64,
    ) -> Result<Vec<AdapterPollAttempt>, SendableError> {
        let rows=sqlx::query(&self.render("SELECT id, adapter_id, adapter_revision, dry_run, state, result, last_error, created_at, updated_at, deadline_at FROM orchestration_adapter_poll_dispatches WHERE adapter_id = ? ORDER BY created_at DESC, id DESC LIMIT ?"))
            .bind(adapter_id).bind(limit.clamp(1,100)).fetch_all(self.pool()).await?;
        rows.into_iter()
            .map(|row| {
                let at =
                    |key| DateTime::<Utc>::from_timestamp(row.get(key), 0).unwrap_or_else(Utc::now);
                Ok(AdapterPollAttempt {
                    id: row.get("id"),
                    adapter_id: row.get("adapter_id"),
                    adapter_revision: row.get("adapter_revision"),
                    dry_run: row.get("dry_run"),
                    state: row.get("state"),
                    result: row
                        .get::<Option<String>, _>("result")
                        .map(|v| serde_json::from_str(&v))
                        .transpose()?
                        .unwrap_or_default(),
                    error: row.get("last_error"),
                    created_at: at("created_at"),
                    updated_at: at("updated_at"),
                    deadline_at: at("deadline_at"),
                })
            })
            .collect()
    }
    async fn finish_adapter_poll_attempt(
        &self,
        id: Uuid,
        state: String,
        result: Value,
        error: Option<String>,
        now: DateTime<Utc>,
    ) -> Result<bool, SendableError> {
        Ok(sqlx::query(&self.render("UPDATE orchestration_adapter_poll_dispatches SET state = ?, result = ?, last_error = ?, updated_at = ? WHERE id = ? AND state NOT IN ('succeeded', 'failed', 'expired')"))
            .bind(state).bind(result.to_string()).bind(error).bind(now.timestamp()).bind(id).execute(self.pool()).await?.affected()>0)
    }
    async fn claim_adapter_poll_publication(
        &self,
        token: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<AdapterPollDispatch>, SendableError> {
        let row=sqlx::query(&self.render("SELECT id FROM orchestration_adapter_poll_dispatches WHERE deadline_at > ? AND (state = 'queued' OR (state = 'publishing' AND publication_until <= ?)) ORDER BY created_at, id LIMIT 1"))
            .bind(now.timestamp()).bind(now.timestamp()).fetch_optional(self.pool()).await?;
        let Some(row) = row else { return Ok(None) };
        let id: Uuid = row.get("id");
        if sqlx::query(&self.render("UPDATE orchestration_adapter_poll_dispatches SET state = 'publishing', publication_token = ?, publication_until = ? WHERE id = ? AND (state = 'queued' OR (state = 'publishing' AND publication_until <= ?))"))
            .bind(token).bind(now.timestamp()+30).bind(id).bind(now.timestamp()).execute(self.pool()).await?.affected()==0{return Ok(None)}
        self.fetch_orchestration_adapter_poll_dispatch(id).await
    }
    async fn finish_adapter_poll_publication(
        &self,
        id: Uuid,
        token: Uuid,
        published: bool,
    ) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE orchestration_adapter_poll_dispatches SET state = ?, updated_at = ?, publication_token = NULL, publication_until = NULL WHERE id = ? AND state = 'publishing' AND publication_token = ?"))
            .bind(if published{"published"}else{"queued"}).bind(Utc::now().timestamp()).bind(id).bind(token).execute(self.pool()).await?;
        Ok(())
    }
    async fn expire_adapter_poll_attempts(&self, now: DateTime<Utc>) -> Result<(), SendableError> {
        sqlx::query(&self.render("UPDATE orchestration_adapter_polls SET last_error = 'poll attempt expired before worker completion', claimed_by = NULL, claimed_until = NULL WHERE claimed_until <= ? AND claimed_by IN (SELECT claim_owner FROM orchestration_adapter_poll_dispatches WHERE dry_run = ? AND state NOT IN ('succeeded', 'failed', 'expired') AND deadline_at <= ?)"))
            .bind(now.timestamp()).bind(false).bind(now.timestamp()).execute(self.pool()).await?;
        sqlx::query(&self.render("UPDATE orchestration_adapter_poll_dispatches SET state = 'expired', last_error = 'poll attempt expired before worker completion', updated_at = ? WHERE deadline_at <= ? AND state NOT IN ('succeeded', 'failed', 'expired')"))
            .bind(now.timestamp()).bind(now.timestamp()).execute(self.pool()).await?;
        Ok(())
    }
    async fn purge_adapter_diagnostics(&self, before: DateTime<Utc>) -> Result<(), SendableError> {
        sqlx::query(&self.render("DELETE FROM adapter_deliveries WHERE updated_at < ? AND state IN ('applied', 'rejected', 'dropped')")).bind(before.timestamp()).execute(self.pool()).await?;
        sqlx::query(&self.render("DELETE FROM orchestration_adapter_poll_dispatches WHERE updated_at < ? AND state IN ('succeeded', 'failed', 'expired')")).bind(before.timestamp()).execute(self.pool()).await?;
        Ok(())
    }
}
