//! packaged functions: published code, its versions, exports, aliases, and artifacts.
//!
//! the `FunctionStore` half of the generic sql implementation. bodies are written once, over any
//! `SqlBackend`; see `super` for the shared helpers they call.

use super::*;

/// every column `mappers::row_to_function_package` reads.
const FUNCTION_PACKAGE_COLUMNS: &str =
    "id, org_id, namespace, name, description, latest_version, archived_at, created_at, updated_at";
const FUNCTION_VERSION_COLUMNS: &str =
    "id, package_id, version, artifact_digest, manifest, runtime, published_by, created_at";
const FUNCTION_EXPORT_COLUMNS: &str =
    "id, version_id, name, handler, description, input, output, limits";
const FUNCTION_ALIAS_COLUMNS: &str =
    "id, package_id, name, version_id, version, created_at, updated_at";
const FUNCTION_ARTIFACT_COLUMNS: &str = "digest, size_bytes, uri, media_type, created_at";
const FUNCTION_ADAPTER_COLUMNS: &str = "id, export_id, workflow_id, created_at";

/// render a package's `(org, namespace, name)` identity into the single column its uniqueness lives
/// on.
///
/// the triple has two nullable halves and every supported engine treats NULLs as distinct in a
/// unique index, so uniqueness has to be carried by a value that is never null. the separator is a
/// character no namespace or name may contain, which is what keeps `a` + `b.c` from colliding with
/// `a.b` + `c`.
fn identity_key(org_id: Option<Uuid>, namespace: Option<&str>, name: &str) -> String {
    let org = org_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "global".to_string());
    format!("{org}\u{1f}{}\u{1f}{name}", namespace.unwrap_or(""))
}

// the bound list is repeated verbatim in every role impl in this directory. it stays spelled out
// rather than hidden behind a macro so that type errors inside the query bodies — the part that
// actually gets edited — keep pointing at real source lines instead of a macro expansion.
impl<B> FunctionStore for SqlStore<B>
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
    async fn upsert_function_artifact(
        &self,
        artifact: &FunctionArtifact,
    ) -> Result<FunctionArtifact, SendableError> {
        // keyed by content, so a repeat publish of identical bytes keeps the original row (and its
        // created_at) rather than rewriting it.
        let conflict = self.dialect().on_conflict_nothing("digest", "digest");
        sqlx::query(&self.render(&format!(
            "INSERT INTO function_artifacts ({FUNCTION_ARTIFACT_COLUMNS}) VALUES (?, ?, ?, ?, ?) {conflict}"
        )))
        .bind(artifact.digest.as_str())
        .bind(artifact.size_bytes)
        .bind(artifact.uri.as_str())
        .bind(artifact.media_type.as_str())
        .bind(artifact.created_at.timestamp())
        .execute(self.pool())
        .await?;
        self.fetch_function_artifact(&artifact.digest)
            .await?
            .ok_or_else(|| crate::errors::FUNCTION_ARTIFACT_MISSING.error(artifact.digest.clone()))
    }

    async fn fetch_function_artifact(
        &self,
        digest: &str,
    ) -> Result<Option<FunctionArtifact>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_ARTIFACT_COLUMNS} FROM function_artifacts WHERE digest = ?"
        )))
        .bind(digest)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_artifact(&row)))
    }

    async fn delete_function_artifact(&self, digest: &str) -> Result<bool, SendableError> {
        // a version pinned to these bytes must stay runnable, so the reference check is here rather
        // than left to a foreign key error the caller would have to interpret.
        let referenced: i64 = sqlx::query_scalar(
            &self.render("SELECT COUNT(*) FROM function_versions WHERE artifact_digest = ?"),
        )
        .bind(digest)
        .fetch_one(self.pool())
        .await?;
        if referenced > 0 {
            return Err(crate::errors::FUNCTION_ARTIFACT_IN_USE
                .error(format!("{digest} is referenced by {referenced} version(s)")));
        }
        let result = sqlx::query(&self.render("DELETE FROM function_artifacts WHERE digest = ?"))
            .bind(digest)
            .execute(self.pool())
            .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_unreferenced_function_artifacts(
        &self,
    ) -> Result<Vec<FunctionArtifact>, SendableError> {
        // a left join rather than `NOT IN`: the three engines disagree about how `NOT IN` treats a
        // null in the subquery, and a null `artifact_digest` would make the whole set empty on one
        // of them — which would silently disable the sweep rather than fail it.
        let rows = sqlx::query(&self.render(&format!(
            "SELECT a.digest, a.size_bytes, a.uri, a.media_type, a.created_at \
             FROM function_artifacts a \
             LEFT JOIN function_versions v ON v.artifact_digest = a.digest \
             WHERE v.id IS NULL ORDER BY a.created_at ASC"
        )))
        .fetch_all(self.pool())
        .await?;
        let _ = FUNCTION_ARTIFACT_COLUMNS;
        Ok(rows.iter().map(mappers::row_to_function_artifact).collect())
    }

    async fn publish_function_version(
        &self,
        request: &NewFunctionVersion,
    ) -> Result<FunctionVersion, SendableError> {
        let now = Utc::now().timestamp();
        let identity = identity_key(
            request.package.org_id,
            request.package.namespace.as_deref(),
            &request.package.name,
        );
        let mut tx = self.pool().begin().await?;

        // upsert the package. the identity key carries the uniqueness, so a concurrent publish of
        // the same package converges on one row instead of racing to create two.
        let package_conflict = self
            .dialect()
            .on_conflict_update("identity_key", &["description", "updated_at"]);
        let package_id = Uuid::now_v7();
        sqlx::query(&self.render(&format!(
            "INSERT INTO function_packages (id, org_id, namespace, name, identity_key, description, latest_version, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, NULL, ?, ?) {package_conflict}"
        )))
        .bind(package_id)
        .bind(request.package.org_id)
        .bind(request.package.namespace.clone())
        .bind(request.package.name.as_str())
        .bind(identity.as_str())
        .bind(request.package.description.clone())
        .bind(now)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        let package_id: Uuid = sqlx::query_scalar(
            &self.render("SELECT id FROM function_packages WHERE identity_key = ?"),
        )
        .bind(identity.as_str())
        .fetch_one(&mut *tx)
        .await?;

        // the version number is assigned here rather than by the publisher: it is what a binding
        // names, so two publishers must never both decide they are version 3.
        let next_version: i64 = sqlx::query_scalar(&self.render(
            "SELECT COALESCE(MAX(version), 0) + 1 FROM function_versions WHERE package_id = ?",
        ))
        .bind(package_id)
        .fetch_one(&mut *tx)
        .await?;

        let version_id = Uuid::now_v7();
        let runtime = serde_json::to_string(&request.runtime)?;
        sqlx::query(&self.render(&format!(
            "INSERT INTO function_versions ({FUNCTION_VERSION_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
        )))
        .bind(version_id)
        .bind(package_id)
        .bind(next_version)
        .bind(request.artifact_digest.as_str())
        .bind(request.manifest.to_string())
        .bind(runtime)
        .bind(Option::<Uuid>::None)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        for export in &request.exports {
            sqlx::query(&self.render(&format!(
                "INSERT INTO function_exports ({FUNCTION_EXPORT_COLUMNS}, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)"
            )))
            .bind(Uuid::now_v7())
            .bind(version_id)
            .bind(export.name.as_str())
            .bind(export.handler.as_str())
            .bind(export.description.clone())
            .bind(serde_json::to_string(&export.input)?)
            .bind(serde_json::to_string(&export.output)?)
            .bind(serde_json::to_string(&export.limits)?)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        if let Some(alias) = &request.alias {
            let alias_conflict = self
                .dialect()
                .on_conflict_update("package_id, name", &["version_id", "version", "updated_at"]);
            sqlx::query(&self.render(&format!(
                "INSERT INTO function_aliases ({FUNCTION_ALIAS_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?) {alias_conflict}"
            )))
            .bind(Uuid::now_v7())
            .bind(package_id)
            .bind(alias.as_str())
            .bind(version_id)
            .bind(next_version)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }

        // `latest_version` is denormalised onto the package so a listing does not join; it tracks
        // the newest published version, not whatever an alias happens to point at.
        sqlx::query(&self.render(
            "UPDATE function_packages SET latest_version = ?, updated_at = ? WHERE id = ?",
        ))
        .bind(next_version)
        .bind(now)
        .bind(package_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.fetch_function_version(version_id)
            .await?
            .ok_or_else(|| crate::errors::FUNCTION_VERSION_MISSING.error(version_id))
    }

    async fn fetch_function_packages(&self) -> Result<Vec<FunctionPackage>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_PACKAGE_COLUMNS} FROM function_packages ORDER BY created_at DESC"
        )))
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_function_package).collect())
    }

    async fn fetch_function_package(
        &self,
        org_id: Option<Uuid>,
        namespace: Option<&str>,
        name: &str,
    ) -> Result<Option<FunctionPackage>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_PACKAGE_COLUMNS} FROM function_packages WHERE identity_key = ?"
        )))
        .bind(identity_key(org_id, namespace, name))
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_package(&row)))
    }

    async fn fetch_function_package_by_id(
        &self,
        package_id: Uuid,
    ) -> Result<Option<FunctionPackage>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_PACKAGE_COLUMNS} FROM function_packages WHERE id = ?"
        )))
        .bind(package_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_package(&row)))
    }

    async fn delete_function_package(&self, package_id: Uuid) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE function_packages SET archived_at = ?, updated_at = ? \
             WHERE id = ? AND archived_at IS NULL",
        ))
        .bind(now)
        .bind(now)
        .bind(package_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn restore_function_package(&self, package_id: Uuid) -> Result<bool, SendableError> {
        let now = Utc::now().timestamp();
        let result = sqlx::query(&self.render(
            "UPDATE function_packages SET archived_at = NULL, updated_at = ? \
             WHERE id = ? AND archived_at IS NOT NULL",
        ))
        .bind(now)
        .bind(package_id)
        .execute(self.pool())
        .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_function_versions(
        &self,
        package_id: Uuid,
    ) -> Result<Vec<FunctionVersion>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_VERSION_COLUMNS} FROM function_versions WHERE package_id = ? ORDER BY version DESC"
        )))
        .bind(package_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_function_version).collect())
    }

    async fn fetch_function_version(
        &self,
        version_id: Uuid,
    ) -> Result<Option<FunctionVersion>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_VERSION_COLUMNS} FROM function_versions WHERE id = ?"
        )))
        .bind(version_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_version(&row)))
    }

    async fn fetch_function_version_by_number(
        &self,
        package_id: Uuid,
        version: i64,
    ) -> Result<Option<FunctionVersion>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_VERSION_COLUMNS} FROM function_versions WHERE package_id = ? AND version = ?"
        )))
        .bind(package_id)
        .bind(version)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_version(&row)))
    }

    async fn fetch_function_exports(
        &self,
        version_id: Uuid,
    ) -> Result<Vec<FunctionExport>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_EXPORT_COLUMNS} FROM function_exports WHERE version_id = ? ORDER BY name"
        )))
        .bind(version_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_function_export).collect())
    }

    async fn fetch_function_export(
        &self,
        export_id: Uuid,
    ) -> Result<Option<FunctionExport>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_EXPORT_COLUMNS} FROM function_exports WHERE id = ?"
        )))
        .bind(export_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_export(&row)))
    }

    async fn set_function_alias(
        &self,
        package_id: Uuid,
        name: &str,
        version_id: Uuid,
    ) -> Result<FunctionAlias, SendableError> {
        // the alias records the version *number* too, so a listing can show `production -> 3`
        // without a join; read it from the version rather than trusting a caller-supplied one.
        let version: i64 = sqlx::query_scalar(
            &self.render("SELECT version FROM function_versions WHERE id = ? AND package_id = ?"),
        )
        .bind(version_id)
        .bind(package_id)
        .fetch_optional(self.pool())
        .await?
        .ok_or_else(|| {
            crate::errors::FUNCTION_VERSION_MISSING.error(format!(
                "{version_id} is not a version of package {package_id}"
            ))
        })?;

        let now = Utc::now().timestamp();
        let conflict = self
            .dialect()
            .on_conflict_update("package_id, name", &["version_id", "version", "updated_at"]);
        sqlx::query(&self.render(&format!(
            "INSERT INTO function_aliases ({FUNCTION_ALIAS_COLUMNS}) VALUES (?, ?, ?, ?, ?, ?, ?) {conflict}"
        )))
        .bind(Uuid::now_v7())
        .bind(package_id)
        .bind(name)
        .bind(version_id)
        .bind(version)
        .bind(now)
        .bind(now)
        .execute(self.pool())
        .await?;

        self.fetch_function_alias(package_id, name)
            .await?
            .ok_or_else(|| crate::errors::FUNCTION_ALIAS_MISSING.error(name.to_string()))
    }

    async fn fetch_function_aliases(
        &self,
        package_id: Uuid,
    ) -> Result<Vec<FunctionAlias>, SendableError> {
        let rows = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_ALIAS_COLUMNS} FROM function_aliases WHERE package_id = ? ORDER BY name"
        )))
        .bind(package_id)
        .fetch_all(self.pool())
        .await?;
        Ok(rows.iter().map(mappers::row_to_function_alias).collect())
    }

    async fn fetch_function_alias(
        &self,
        package_id: Uuid,
        name: &str,
    ) -> Result<Option<FunctionAlias>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_ALIAS_COLUMNS} FROM function_aliases WHERE package_id = ? AND name = ?"
        )))
        .bind(package_id)
        .bind(name)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_alias(&row)))
    }

    async fn delete_function_alias(
        &self,
        package_id: Uuid,
        name: &str,
    ) -> Result<bool, SendableError> {
        let result = retry_delete(|| async {
            sqlx::query(
                &self.render("DELETE FROM function_aliases WHERE package_id = ? AND name = ?"),
            )
            .bind(package_id)
            .bind(name)
            .execute(self.pool())
            .await
        })
        .await?;
        Ok(result.affected() > 0)
    }

    async fn fetch_function_catalog(&self) -> Result<Vec<FunctionCatalogEntry>, SendableError> {
        let rows = sqlx::query(&self.render(
            "SELECT p.id AS package_id, p.name AS package_name, p.namespace AS namespace, \
                    v.id AS version_id, v.version AS version, v.artifact_digest AS artifact_digest, \
                    e.id AS export_id, e.name AS export_name, e.description AS description, \
                    e.input AS input, e.output AS output \
             FROM function_exports e \
             JOIN function_versions v ON v.id = e.version_id \
             JOIN function_packages p ON p.id = v.package_id \
             WHERE p.archived_at IS NULL \
             ORDER BY p.name, v.version DESC, e.name",
        ))
        .fetch_all(self.pool())
        .await?;
        let mut entries: Vec<FunctionCatalogEntry> = rows
            .iter()
            .map(mappers::row_to_function_catalog_entry)
            .collect();

        // aliases are attached in a second pass rather than joined: a version may carry several, and
        // joining them would multiply every export row by its version's alias count.
        let alias_rows = sqlx::query(
            &self.render("SELECT version_id, name FROM function_aliases ORDER BY name"),
        )
        .fetch_all(self.pool())
        .await?;
        let mut aliases: HashMap<Uuid, Vec<String>> = HashMap::new();
        for row in &alias_rows {
            aliases
                .entry(row.get("version_id"))
                .or_default()
                .push(row.get("name"));
        }
        for entry in &mut entries {
            if let Some(names) = aliases.get(&entry.version_id) {
                entry.aliases = names.clone();
            }
        }
        Ok(entries)
    }

    async fn upsert_function_adapter_workflow(
        &self,
        export_id: Uuid,
        workflow_id: Uuid,
    ) -> Result<FunctionAdapterWorkflow, SendableError> {
        let now = Utc::now().timestamp();
        let conflict = self
            .dialect()
            .on_conflict_update("export_id", &["workflow_id"]);
        sqlx::query(&self.render(&format!(
            "INSERT INTO function_adapter_workflows ({FUNCTION_ADAPTER_COLUMNS}) VALUES (?, ?, ?, ?) {conflict}"
        )))
        .bind(Uuid::now_v7())
        .bind(export_id)
        .bind(workflow_id)
        .bind(now)
        .execute(self.pool())
        .await?;
        self.fetch_function_adapter_workflow(export_id)
            .await?
            .ok_or_else(|| crate::errors::FUNCTION_ADAPTER_MISSING.error(export_id))
    }

    async fn fetch_function_adapter_workflow(
        &self,
        export_id: Uuid,
    ) -> Result<Option<FunctionAdapterWorkflow>, SendableError> {
        let row = sqlx::query(&self.render(&format!(
            "SELECT {FUNCTION_ADAPTER_COLUMNS} FROM function_adapter_workflows WHERE export_id = ?"
        )))
        .bind(export_id)
        .fetch_optional(self.pool())
        .await?;
        Ok(row.map(|row| mappers::row_to_function_adapter_workflow(&row)))
    }
}

#[cfg(test)]
#[path = "functions_tests.rs"]
mod tests;
