//! VM-native user-file metadata operations.

use super::*;

const FILE_COLUMNS: &str = "id, scope, org_id, owner_id, workflow_run_id, path, name, mime_type, size_bytes, sha256, uri, revision, is_current, archived, created_at";

impl<B> FileStore for SqlStore<B>
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
    for<'r> Uuid: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Option<Uuid>: Decode<'r, B::Db> + Type<B::Db>,
    for<'r> Vec<u8>: Decode<'r, B::Db> + Type<B::Db>,
    usize: ColumnIndex<<B::Db as Database>::Row>,
    for<'c> &'c str: ColumnIndex<<B::Db as Database>::Row>,
    for<'q> <B::Db as Database>::Arguments<'q>: IntoArguments<'q, B::Db>,
    for<'c> &'c mut <B::Db as Database>::Connection: Executor<'c, Database = B::Db>,
    <B::Db as Database>::QueryResult: RowsAffected,
{
    async fn insert_file(&self, file: &StoredFile) -> Result<StoredFile, SendableError> {
        let mut tx = self.pool().begin().await?;
        if file.scope == FileScope::Library && file.current {
            sqlx::query(&self.render(
                "UPDATE workflow_files SET is_current = ? WHERE scope = 'library' AND path = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))",
            ))
            .bind(false)
            .bind(&file.descriptor.path)
            .bind(file.org_id)
            .bind(file.org_id)
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(&self.render(&format!(
            "INSERT INTO workflow_files ({FILE_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(file.descriptor.id)
        .bind(file.scope.as_str())
        .bind(file.org_id)
        .bind(file.owner_id)
        .bind(file.workflow_run_id)
        .bind(&file.descriptor.path)
        .bind(&file.descriptor.name)
        .bind(&file.descriptor.mime_type)
        .bind(file.descriptor.size_bytes)
        .bind(&file.descriptor.sha256)
        .bind(&file.uri)
        .bind(file.revision)
        .bind(file.current)
        .bind(file.archived)
        .bind(file.created_at.timestamp())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(file.clone())
    }

    async fn fetch_file(&self, id: Uuid) -> Result<Option<StoredFile>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FILE_COLUMNS} FROM workflow_files WHERE id = ?"
        )))
        .bind(id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.as_ref().map(mappers::row_to_stored_file))
    }

    async fn list_library_files(
        &self,
        org_id: Option<Uuid>,
    ) -> Result<Vec<StoredFile>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {FILE_COLUMNS} FROM workflow_files WHERE scope = 'library' AND is_current = ? AND archived = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL)) ORDER BY path ASC"
        )))
        .bind(true)
        .bind(false)
        .bind(org_id)
        .bind(org_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_stored_file).collect())
    }

    async fn next_library_revision(
        &self,
        org_id: Option<Uuid>,
        path: &str,
    ) -> Result<i64, SendableError> {
        let revision: Option<i64> = sqlx::query_scalar(&self.render(
            "SELECT MAX(revision) FROM workflow_files WHERE scope = 'library' AND path = ? AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))",
        ))
        .bind(path)
        .bind(org_id)
        .bind(org_id)
        .fetch_one(self.pool())
        .await?;
        Ok(revision.unwrap_or(0) + 1)
    }

    async fn claim_staged_files(
        &self,
        ids: &[Uuid],
        org_id: Option<Uuid>,
        owner_id: Option<Uuid>,
        workflow_run_id: Uuid,
    ) -> Result<Vec<StoredFile>, SendableError> {
        let mut claimed = Vec::with_capacity(ids.len());
        let mut tx = self.pool().begin().await?;
        for id in ids {
            let existing = sqlx::query(&self.render(&format!(
                "SELECT {FILE_COLUMNS} FROM workflow_files WHERE id = ?"
            )))
            .bind(*id)
            .fetch_optional(&mut *tx)
            .await?;
            let Some(existing) = existing else {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "selected workflow file no longer exists",
                )));
            };
            let existing = mappers::row_to_stored_file(&existing);
            if existing.scope == FileScope::Library
                && !existing.archived
                && existing.org_id == org_id
            {
                claimed.push(existing);
                continue;
            }
            let changed = sqlx::query(&self.render(
                "UPDATE workflow_files SET scope = 'run', workflow_run_id = ?, owner_id = NULL WHERE id = ? AND scope = 'staged' AND (org_id = ? OR (org_id IS NULL AND ? IS NULL)) AND (owner_id = ? OR (owner_id IS NULL AND ? IS NULL))",
            ))
            .bind(workflow_run_id)
            .bind(*id)
            .bind(org_id)
            .bind(org_id)
            .bind(owner_id)
            .bind(owner_id)
            .execute(&mut *tx)
            .await?;
            if changed.affected() != 1 {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "staged file is unavailable or not owned by this principal",
                )));
            }
            let row = sqlx::query(&self.render(&format!(
                "SELECT {FILE_COLUMNS} FROM workflow_files WHERE id = ?"
            )))
            .bind(*id)
            .fetch_one(&mut *tx)
            .await?;
            claimed.push(mappers::row_to_stored_file(&row));
        }
        tx.commit().await?;
        Ok(claimed)
    }

    async fn archive_file(&self, id: Uuid, org_id: Option<Uuid>) -> Result<bool, SendableError> {
        let result = sqlx::query(&self.render(
            "UPDATE workflow_files SET archived = ? WHERE id = ? AND scope = 'library' AND (org_id = ? OR (org_id IS NULL AND ? IS NULL))",
        ))
        .bind(true)
        .bind(id)
        .bind(org_id)
        .bind(org_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() == 1)
    }
}
