use super::*;

const AI_USAGE_COLUMNS: &str = "event_id, effect_id, workflow_run_id, workflow_id, node_id, attempt, provider, model, input_tokens, cached_input_tokens, cache_creation_input_tokens, output_tokens, reasoning_tokens, cost_microusd, cost_source, recorded_at";

fn row_to_ai_usage<R>(row: &R) -> AiUsageRecord
where
    R: Row,
    for<'c> &'c str: ColumnIndex<R>,
    for<'d> i64: Decode<'d, R::Database> + Type<R::Database>,
    for<'d> String: Decode<'d, R::Database> + Type<R::Database>,
    for<'d> Uuid: Decode<'d, R::Database> + Type<R::Database>,
    for<'d> Option<i64>: Decode<'d, R::Database> + Type<R::Database>,
    for<'d> Option<String>: Decode<'d, R::Database> + Type<R::Database>,
{
    AiUsageRecord {
        event_id: row.get("event_id"),
        effect_id: row.get("effect_id"),
        workflow_run_id: row.get("workflow_run_id"),
        workflow_id: row.get("workflow_id"),
        node_id: row.get("node_id"),
        attempt: u32::try_from(row.get::<i64, _>("attempt")).unwrap_or_default(),
        provider: row.get("provider"),
        model: row.get("model"),
        tokens: AiTokenUsage {
            input_tokens: u64::try_from(row.get::<i64, _>("input_tokens")).unwrap_or_default(),
            cached_input_tokens: u64::try_from(row.get::<i64, _>("cached_input_tokens"))
                .unwrap_or_default(),
            cache_creation_input_tokens: u64::try_from(
                row.get::<i64, _>("cache_creation_input_tokens"),
            )
            .unwrap_or_default(),
            output_tokens: u64::try_from(row.get::<i64, _>("output_tokens")).unwrap_or_default(),
            reasoning_tokens: u64::try_from(row.get::<i64, _>("reasoning_tokens"))
                .unwrap_or_default(),
        },
        cost_microusd: row
            .get::<Option<i64>, _>("cost_microusd")
            .and_then(|value| u64::try_from(value).ok()),
        cost_source: row
            .get::<Option<String>, _>("cost_source")
            .as_deref()
            .and_then(|value| AiCostSource::try_from(value).ok()),
        recorded_at: DateTime::<Utc>::from_timestamp(row.get("recorded_at"), 0)
            .unwrap_or_else(Utc::now),
    }
}

impl<B> AiUsageStore for SqlStore<B>
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> String: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<i64>: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Option<String>: Encode<'q, B::Db> + Type<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    <B::Db as Database>::Arguments: IntoArguments<B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn insert_ai_usage(&self, record: AiUsageRecord) -> Result<bool, SendableError> {
        let sql = self.dialect().insert_ignore(
            "workflow_ai_usage",
            "event_id, effect_id, workflow_run_id, workflow_id, node_id, attempt, provider, model, input_tokens, cached_input_tokens, cache_creation_input_tokens, output_tokens, reasoning_tokens, cost_microusd, cost_source, recorded_at",
            "?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?",
            "event_id",
            None,
        );
        let result = sqlx::query(&self.render(&sql))
            .bind(record.event_id)
            .bind(record.effect_id)
            .bind(record.workflow_run_id)
            .bind(record.workflow_id)
            .bind(record.node_id)
            .bind(i64::from(record.attempt))
            .bind(record.provider)
            .bind(record.model)
            .bind(i64::try_from(record.tokens.input_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(record.tokens.cached_input_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(record.tokens.cache_creation_input_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(record.tokens.output_tokens).unwrap_or(i64::MAX))
            .bind(i64::try_from(record.tokens.reasoning_tokens).unwrap_or(i64::MAX))
            .bind(
                record
                    .cost_microusd
                    .and_then(|value| i64::try_from(value).ok()),
            )
            .bind(record.cost_source.map(|source| source.as_str().to_owned()))
            .bind(record.recorded_at.timestamp())
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_ai_usage_for_run(
        &self,
        workflow_run_id: Uuid,
    ) -> Result<Vec<AiUsageRecord>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {AI_USAGE_COLUMNS} FROM workflow_ai_usage WHERE workflow_run_id = ? ORDER BY recorded_at, event_id"
        )))
        .bind(workflow_run_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(row_to_ai_usage).collect())
    }

    async fn fetch_ai_usage_for_workflow(
        &self,
        workflow_id: Uuid,
        since: Option<DateTime<Utc>>,
        until: Option<DateTime<Utc>>,
    ) -> Result<Vec<AiUsageRecord>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {AI_USAGE_COLUMNS} FROM workflow_ai_usage WHERE workflow_id = ? AND (? IS NULL OR recorded_at >= ?) AND (? IS NULL OR recorded_at <= ?) ORDER BY recorded_at, event_id"
        )))
        .bind(workflow_id)
        .bind(since.map(|value| value.timestamp()))
        .bind(since.map(|value| value.timestamp()))
        .bind(until.map(|value| value.timestamp()))
        .bind(until.map(|value| value.timestamp()))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(row_to_ai_usage).collect())
    }
}
