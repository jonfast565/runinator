#[allow(unused_imports)]
use super::*;

pub(super) trait ArchiveSqlExt: SqlBackend {
    async fn archive_candidate_ids(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<(Uuid, DateTime<Utc>)>, SendableError>;

    async fn fetch_archive_row(
        &self,
        mark: &ArchiveMark,
    ) -> Result<Option<ArchiveRow>, SendableError>;
}

impl<B> ArchiveSqlExt for B
where
    B: SqlBackend,
    for<'q> i64: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> Uuid: Encode<'q, B::Db> + Type<B::Db>,
    for<'q> &'q str: Encode<'q, B::Db> + Type<B::Db>,
    <B::Db as Database>::Arguments: IntoArguments<B::Db>,
    for<'r> i64: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> String: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> bool: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<i64>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<String>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
{
    async fn archive_candidate_ids(
        &self,
        table: ArchiveTable,
        eligible_before: DateTime<Utc>,
        limit: i64,
    ) -> Result<Vec<(Uuid, DateTime<Utc>)>, SendableError> {
        let sql = table.archive_candidate_sql();
        let rows = sqlx::query(&self.render(sql))
            .bind(eligible_before.timestamp())
            .bind(limit)
            .fetch_all(self.pool())
            .await?;
        rows.iter()
            .map(|row| {
                let id: Uuid = row.get("id");
                let created_at: i64 = row.get("created_at");
                Ok((id, timestamp_to_utc(created_at)?))
            })
            .collect()
    }

    async fn fetch_archive_row(
        &self,
        mark: &ArchiveMark,
    ) -> Result<Option<ArchiveRow>, SendableError> {
        let Some(row) =
            sqlx::query(&self.render(&mark.table.archive_source_sql_v2(self.dialect())))
                .bind(mark.primary_key)
                .fetch_optional(self.pool())
                .await?
        else {
            return Ok(None);
        };
        let row_json = mark.table.archive_row_json_v2(&row)?;
        Ok(Some(ArchiveRow {
            mark_id: mark.id,
            table: mark.table,
            primary_key: mark.primary_key,
            created_at: mark.created_at,
            row: row_json,
        }))
    }
}
