mod connector;

use std::collections::HashMap;
use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use connector::DatabaseConnector;
use connector::postgres::PostgresConnector;
use log::info;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    providers::{
        ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata,
        ProviderRuntimeMetadata, ResultMetadata,
    },
    runs::{NewRunArtifact, ProviderExecutionRequest, TaskExecutionResult},
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use runinator_utilities::data_export::{
    TableExportContext, TableExporter, csv::CsvTableExporter, excel::ExcelTableExporter,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct SqlProvider;

impl Provider for SqlProvider {
    fn name(&self) -> String {
        "SQL".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                ActionMetadata::new(
                    "dump_data",
                    "Execute SQL queries and export results to Excel/CSV",
                )
                .with_parameters(vec![
                    ParameterMetadata::required("database", ParameterValueType::String),
                    ParameterMetadata::required("connection_string", ParameterValueType::String)
                        .secret(),
                    ParameterMetadata::required("dump_folder", ParameterValueType::String),
                    ParameterMetadata::required("queries", ParameterValueType::Object),
                    ParameterMetadata::optional("file_prefix", ParameterValueType::String),
                    ParameterMetadata::optional("format", ParameterValueType::String)
                        .with_default(json!("excel")),
                ])
                .with_results(vec![
                    ResultMetadata::new("provider", ParameterValueType::String),
                    ResultMetadata::new("exports", ParameterValueType::Object),
                ]),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["sql".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
    ) -> Result<TaskExecutionResult, SendableError> {
        match request.action_function.as_str() {
            "dump_data" => self.dump_data(request.parameters, request.timeout_secs),
            _ => Err(Box::new(RuntimeError::new(
                "UNSUPPORTED_CALL".to_string(),
                format!(
                    "Unsupported SQL provider call '{}'",
                    request.action_function
                ),
            ))),
        }
    }
}

impl SqlProvider {
    fn dump_data(
        &self,
        parameters: serde_json::Value,
        timeout_secs: i64,
    ) -> Result<TaskExecutionResult, SendableError> {
        let request: DumpDataRequest =
            serde_json::from_value(parameters).map_err(|err| to_sendable(err))?;

        if request.queries.is_empty() {
            return Err(Box::new(RuntimeError::new(
                "INVALID_ARGUMENT".to_string(),
                "At least one query must be provided".to_string(),
            )));
        }

        let timeout = normalize_timeout(timeout_secs);
        let dump_dir = PathBuf::from(&request.dump_folder);
        fs::create_dir_all(&dump_dir).map_err(|err| to_sendable(err))?;

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
            info!("Executing query {} for database dump", idx + 1);
            let table_data = connector.execute_query(&query.sql, timeout)?;

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

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum DatabaseKind {
    Postgres,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum DumpFormat {
    Excel,
    Csv,
}

impl Default for DumpFormat {
    fn default() -> Self {
        DumpFormat::Excel
    }
}

impl DumpFormat {
    fn file_extension(&self) -> &'static str {
        match self {
            DumpFormat::Excel => "xlsx",
            DumpFormat::Csv => "csv",
        }
    }

    fn requires_sheet_name(&self) -> bool {
        matches!(self, DumpFormat::Excel)
    }

    fn mime_type(&self) -> &'static str {
        match self {
            DumpFormat::Excel => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            DumpFormat::Csv => "text/csv",
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            DumpFormat::Excel => "excel",
            DumpFormat::Csv => "csv",
        }
    }
}

struct SqlExport {
    name: String,
    rows: usize,
    path: PathBuf,
    mime_type: String,
    size_bytes: i64,
    format: DumpFormat,
}

#[derive(Deserialize)]
struct DumpDataRequest {
    database: DatabaseKind,
    connection_string: String,
    dump_folder: String,
    queries: Vec<QueryConfig>,
    #[serde(default)]
    file_prefix: Option<String>,
    #[serde(default)]
    format: DumpFormat,
}

#[derive(Deserialize)]
struct QueryConfig {
    sql: String,
    #[serde(default)]
    name: Option<String>,
}

fn normalize_timeout(timeout_secs: i64) -> Duration {
    if timeout_secs <= 0 {
        Duration::from_secs(30)
    } else {
        Duration::from_secs(timeout_secs as u64)
    }
}

fn sanitize_file_stem(input: &str) -> String {
    let mut sanitized = input
        .chars()
        .map(|ch| match ch {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            _ if ch.is_control() => '_',
            _ => ch,
        })
        .collect::<String>();

    sanitized = sanitized
        .trim()
        .trim_matches('.')
        .trim_matches('\'')
        .to_string();

    if sanitized.is_empty() {
        return sanitized;
    }

    const MAX_LEN: usize = 120;
    if sanitized.len() > MAX_LEN {
        sanitized.truncate(MAX_LEN);
    }

    sanitized
}

fn next_available_stem(base: String, counts: &mut HashMap<String, usize>) -> String {
    let counter = counts.entry(base.clone()).or_insert(0usize);
    let stem = if base.is_empty() {
        format!("query_{:02}", *counter + 1)
    } else if *counter == 0 {
        base.clone()
    } else {
        format!("{base}_{:02}", *counter)
    };
    *counter += 1;
    stem
}

fn to_sendable<E>(err: E) -> SendableError
where
    E: Error + Send + Sync + 'static,
{
    Box::new(err)
}

fn file_size(path: &PathBuf) -> Result<i64, SendableError> {
    Ok(fs::metadata(path).map_err(|err| to_sendable(err))?.len() as i64)
}
