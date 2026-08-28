//! aging rows out to cold storage.
//!
//! the `ArchiveStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> ArchiveStore for SqlStore<B>
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
    async fn mark_archive_candidates(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<u64, SendableError> {
        let candidates = self
            .archive_candidate_ids(table, eligible_before, limit.max(1))
            .await?;
        let now = Utc::now().timestamp();
        let archive_day = eligible_before.format("%F").to_string();
        let mut marked = 0;
        for (primary_key, created_at) in candidates {
            let insert = sqlx::query(&self.render(&self.dialect().insert_ignore(
                "archive_marks",
                "id, table_name, primary_key, created_at, eligible_before, archive_day, status, attempts, marked_at",
                "?, ?, ?, ?, ?, ?, 'marked', 0, ?",
                "table_name, primary_key",
                None,
            )))
            .bind(Uuid::now_v7())
            .bind(table.as_str())
            .bind(primary_key.to_string())
            .bind(created_at.timestamp())
            .bind(eligible_before.timestamp())
            .bind(archive_day.as_str())
            .bind(now)
            .execute(self.pool())
            .await?;
            marked += insert.affected();
        }
        Ok(marked)
    }

    async fn claim_archive_marks(
        &self,
        archiver_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<ArchiveMark>, SendableError> {
        let columns = "id, table_name, primary_key, created_at, eligible_before, archive_day";
        if self.dialect() == SqlDialect::MariaDb {
            sqlx::query(&self.render(
                "UPDATE archive_marks
                 SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1
                 WHERE id IN (
                     SELECT id FROM (
                         SELECT id FROM archive_marks
                         WHERE status = 'marked'
                           AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                         ORDER BY archive_day, table_name, primary_key
                         LIMIT ?
                     ) AS claimable
                 )",
            ))
            .bind(archiver_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(archiver_id.as_str())
            .bind(limit.max(1))
            .execute(self.pool())
            .await?;
            let rows = sqlx::query(&self.render(&format!(
                "SELECT {columns} FROM archive_marks WHERE claimed_by = ? AND claimed_until = ? ORDER BY archive_day, table_name, primary_key",
            )))
            .bind(archiver_id.as_str())
            .bind(lease_until.timestamp())
            .fetch_all(self.pool())
            .await?;
            return rows.iter().map(row_to_archive_mark).collect();
        }

        let sql = self.render(&format!(
            "UPDATE archive_marks
             SET claimed_by = ?, claimed_until = ?, attempts = attempts + 1
             WHERE id IN (
                 SELECT id FROM archive_marks
                 WHERE status = 'marked'
                   AND (claimed_until IS NULL OR claimed_until <= ? OR claimed_by = ?)
                 ORDER BY archive_day, table_name, primary_key
                 LIMIT ?{skip}
             )
             RETURNING {columns}",
            skip = self.dialect().skip_locked(),
        ));
        let rows = sqlx::query(&sql)
            .bind(archiver_id.as_str())
            .bind(lease_until.timestamp())
            .bind(now.timestamp())
            .bind(archiver_id.as_str())
            .bind(limit.max(1))
            .fetch_all(self.pool())
            .await?;
        rows.iter().map(row_to_archive_mark).collect()
    }

    async fn fetch_archive_rows(
        &self,
        marks: Vec<ArchiveMark>,
    ) -> Result<Vec<ArchiveRow>, SendableError> {
        let mut rows = Vec::new();
        for mark in marks {
            if let Some(row) = self.fetch_archive_row(&mark).await? {
                rows.push(row);
            }
        }
        Ok(rows)
    }

    async fn delete_archive_rows(&self, rows: Vec<ArchiveRow>) -> Result<u64, SendableError> {
        Ok(retry_delete(|| async {
            let mut tx = self.pool().begin().await?;
            let mut deleted = 0;
            for row in &rows {
                let sql = format!(
                    "DELETE FROM {} WHERE {} = ?",
                    row.table.as_str(),
                    row.table.primary_key_column()
                );
                let result = sqlx::query(&self.render(&sql))
                    .bind(row.primary_key)
                    .execute(&mut *tx)
                    .await?;
                deleted += result.affected();
            }
            tx.commit().await?;
            Ok(deleted)
        })
        .await?)
    }

    async fn complete_archive_marks(&self, mark_ids: Vec<Uuid>) -> Result<u64, SendableError> {
        let now = Utc::now().timestamp();
        let mut updated = 0;
        for mark_id in mark_ids {
            let result = sqlx::query(&self.render(
                "UPDATE archive_marks
                 SET status = 'archived', archived_at = ?, claimed_by = NULL, claimed_until = NULL, last_error = NULL
                 WHERE id = ?",
            ))
            .bind(now)
            .bind(mark_id)
            .execute(self.pool())
            .await?;
            updated += result.affected();
        }
        Ok(updated)
    }

    async fn fail_archive_marks(
        &self,
        mark_ids: Vec<Uuid>,
        error: String,
    ) -> Result<u64, SendableError> {
        let mut updated = 0;
        for mark_id in mark_ids {
            let result = sqlx::query(&self.render(
                "UPDATE archive_marks
                 SET claimed_by = NULL, claimed_until = NULL, last_error = ?
                 WHERE id = ? AND status = 'marked'",
            ))
            .bind(error.as_str())
            .bind(mark_id)
            .execute(self.pool())
            .await?;
            updated += result.affected();
        }
        Ok(updated)
    }

    async fn prune_completed_archive_marks(
        &self,
        archived_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<u64, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT id FROM archive_marks WHERE status = 'archived' AND archived_at <= ? ORDER BY archived_at, id LIMIT ?",
        ))
        .bind(archived_before.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        let mut deleted = 0;
        for row in rows {
            deleted += sqlx::query(&self.render("DELETE FROM archive_marks WHERE id = ?"))
                .bind(row.get::<Uuid, _>("id"))
                .execute(self.pool())
                .await?
                .affected();
        }
        Ok(deleted)
    }

    async fn prune_expired_security_records(
        &self,
        expired_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<u64, SendableError> {
        let limit = limit.max(1);
        let sessions = sqlx::query(&self.render(
            "SELECT id FROM auth_sessions WHERE expires_at <= ? OR revoked = ? ORDER BY expires_at, id LIMIT ?",
        ))
        .bind(expired_before.timestamp())
        .bind(true)
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        let mut deleted = 0;
        for row in sessions {
            deleted += sqlx::query(&self.render("DELETE FROM auth_sessions WHERE id = ?"))
                .bind(row.get::<Uuid, _>("id"))
                .execute(self.pool())
                .await?
                .affected();
        }
        let tokens = sqlx::query(&self.render(
            "SELECT token_id FROM agent_enrollment_tokens WHERE expires_at <= ? OR consumed_at IS NOT NULL ORDER BY expires_at, token_id LIMIT ?",
        ))
        .bind(expired_before.timestamp())
        .bind(limit)
        .fetch_all(self.pool())
        .await?;
        for row in tokens {
            deleted +=
                sqlx::query(&self.render("DELETE FROM agent_enrollment_tokens WHERE token_id = ?"))
                    .bind(row.get::<String, _>("token_id"))
                    .execute(self.pool())
                    .await?
                    .affected();
        }
        Ok(deleted)
    }

    async fn prune_workflow_cooldowns(
        &self,
        used_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<u64, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT name FROM workflow_cooldowns WHERE last_run_at <= ? ORDER BY last_run_at, name LIMIT ?",
        ))
        .bind(used_before.timestamp())
        .bind(limit.max(1))
        .fetch_all(self.pool())
        .await?;
        let mut deleted = 0;
        for row in rows {
            deleted += sqlx::query(&self.render("DELETE FROM workflow_cooldowns WHERE name = ?"))
                .bind(row.get::<String, _>("name"))
                .execute(self.pool())
                .await?
                .affected();
        }
        Ok(deleted)
    }
}
