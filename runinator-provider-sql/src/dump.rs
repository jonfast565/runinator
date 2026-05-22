use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use log::info;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{NewRunArtifact, TaskExecutionResult},
};
use runinator_plugin::cancel::CancellationToken;
use runinator_utilities::data_export::{
    TableExportContext, TableExporter, csv::CsvTableExporter, excel::ExcelTableExporter,
};
use serde::Deserialize;
use serde_json::json;

use crate::SqlProvider;
use crate::connector::DatabaseConnector;
use crate::connector::postgres::PostgresConnector;
use crate::format::{DatabaseKind, DumpFormat};
use crate::helpers::{
    file_size, next_available_stem, normalize_timeout, sanitize_file_stem, to_sendable,
};

pub(crate) struct SqlExport {
    pub name: String,
    pub rows: usize,
    pub path: PathBuf,
    pub mime_type: String,
    pub size_bytes: i64,
    pub format: DumpFormat,
}

#[derive(Deserialize)]
pub(crate) struct DumpDataRequest {
    pub database: DatabaseKind,
    pub connection_string: String,
    pub dump_folder: String,
    pub queries: Vec<QueryConfig>,
    #[serde(default)]
    pub file_prefix: Option<String>,
    #[serde(default)]
    pub format: DumpFormat,
}

#[derive(Deserialize)]
pub(crate) struct QueryConfig {
    pub sql: String,
    #[serde(default)]
    pub name: Option<String>,
}

impl SqlProvider {
    pub(crate) fn dump_data(
        &self,
        parameters: serde_json::Value,
        timeout_secs: i64,
        token: CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let request: DumpDataRequest = serde_json::from_value(parameters).map_err(to_sendable)?;

        if request.queries.is_empty() {
            return Err(Box::new(RuntimeError::new(
                "INVALID_ARGUMENT".to_string(),
                "At least one query must be provided".to_string(),
            )));
        }

        let timeout = normalize_timeout(timeout_secs);
        let dump_dir = PathBuf::from(&request.dump_folder);
        fs::create_dir_all(&dump_dir).map_err(to_sendable)?;

        let connector: Box<dyn DatabaseConnector> = match request.database {
            DatabaseKind::Postgres => Box::new(PostgresConnector::new(request.connection_string)),
        };

        let mut file_counts: HashMap<String, usize> = HashMap::new();
        let format = request.format;
        let exporter: Box<dyn TableExporter> = match format {
            DumpFormat::Excel => Box::new(ExcelTableExporter::new()),
            DumpFormat::Csv => Box::new(CsvTableExporter::new()),
        };
        let mut exports = Vec::new();

        for (idx, query) in request.queries.iter().enumerate() {
            if token.is_cancelled() {
                return Err(Box::new(RuntimeError::new(
                    "QUERY_CANCELED".to_string(),
                    "SQL dump canceled".to_string(),
                )));
            }
            info!("Executing query {} for database dump", idx + 1);
            let table_data = connector.execute_query(&query.sql, timeout)?;
            if token.is_cancelled() {
                return Err(Box::new(RuntimeError::new(
                    "QUERY_CANCELED".to_string(),
                    "SQL dump canceled".to_string(),
                )));
            }

            let default_stem = format!("query_{:02}", idx + 1);
            let query_stem = query
                .name
                .as_deref()
                .map(sanitize_file_stem)
                .filter(|stem| !stem.is_empty())
                .unwrap_or(default_stem);

            let file_prefix = request.file_prefix.as_deref().unwrap_or("");
            let combined_stem = format!("{file_prefix}{query_stem}");
            let unique_stem = next_available_stem(combined_stem, &mut file_counts);
            let file_path = dump_dir.join(format!("{unique_stem}.{}", format.file_extension()));

            let sheet_name_owned = query
                .name
                .clone()
                .unwrap_or_else(|| format!("Query {}", idx + 1));

            let sheet_name_ref = if format.requires_sheet_name() {
                Some(sheet_name_owned.as_str())
            } else {
                None
            };
            let context = TableExportContext {
                sheet_name: sheet_name_ref,
            };

            exporter.export(&file_path, &table_data, &context)?;
            let size_bytes = file_size(&file_path)?;
            exports.push(SqlExport {
                name: sheet_name_owned.clone(),
                rows: table_data.rows.len(),
                path: file_path.clone(),
                mime_type: format.mime_type().to_string(),
                size_bytes,
                format,
            });
            info!(
                "Wrote {} rows to {}",
                table_data.rows.len(),
                file_path.display()
            );
        }

        let artifacts = exports
            .iter()
            .map(|export| NewRunArtifact {
                name: export
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| export.name.clone()),
                mime_type: export.mime_type.clone(),
                size_bytes: export.size_bytes,
                uri: export.path.to_string_lossy().into_owned(),
                metadata: json!({
                    "provider": "SQL",
                    "query_name": export.name,
                    "rows": export.rows,
                    "format": export.format.as_str(),
                }),
            })
            .collect::<Vec<_>>();

        Ok(TaskExecutionResult {
            message: Some(format!("Exported {} SQL result file(s)", artifacts.len())),
            output_json: Some(json!({
                "provider": "SQL",
                "exports": exports.iter().map(|export| {
                    json!({
                        "name": export.name,
                        "rows": export.rows,
                        "path": export.path,
                        "format": export.format.as_str(),
                        "size_bytes": export.size_bytes,
                    })
                }).collect::<Vec<_>>()
            })),
            chunks: Vec::new(),
            artifacts,
        })
    }
}
