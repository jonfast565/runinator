use std::collections::HashMap;

use runinator_models::errors::SendableError;
use runinator_models::runs::TaskExecutionResult;
use runinator_plugin::cancel::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{Shape, check_cancelled, open};
use crate::engine::Engine;
use crate::export::{ExportSpec, export_rows};
use crate::helpers::{build_runtime, normalize_timeout, to_sendable};
use crate::statement::{StatementFields, StatementSpec};

#[derive(Deserialize)]
pub struct QueryRequest {
    pub engine: Engine,
    pub connection: String,
    #[serde(flatten)]
    pub statement: StatementFields,
    #[serde(default)]
    pub shape: Shape,
    #[serde(default)]
    pub export: Option<ExportSpec>,
}

/// run one row-returning statement and hand the rows back in the requested shape, optionally
/// also writing them to a spreadsheet.
pub fn run(
    parameters: Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let request: QueryRequest = serde_json::from_value(parameters).map_err(to_sendable)?;
    let timeout = normalize_timeout(timeout_secs);
    let statement = StatementSpec::resolve(request.statement, request.engine)?;

    check_cancelled(&token)?;
    let runtime = std::sync::Arc::new(build_runtime()?);
    let connector = open(request.engine, &request.connection, runtime)?;
    let rows = connector.query(&statement, timeout)?;
    check_cancelled(&token)?;

    let mut output = match request.shape.project(&rows) {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("result".to_string(), other);
            map
        }
    };
    output.insert("provider".to_string(), json!("db"));
    output.insert("engine".to_string(), json!(request.engine.as_str()));
    output.insert("shape".to_string(), json!(request.shape.as_str()));

    let mut artifacts = Vec::new();
    if let Some(spec) = &request.export {
        let mut counts = HashMap::new();
        let fallback = statement.name().unwrap_or("query");
        let exported = export_rows(&rows, spec, fallback, 0, &mut counts)?;
        output.insert("exports".to_string(), json!([exported.to_json()]));
        artifacts.push(exported.to_artifact());
    }

    Ok(TaskExecutionResult {
        message: Some(format!("Returned {} row(s)", rows.row_count())),
        output_json: Some(Value::Object(output).into()),
        chunks: Vec::new(),
        artifacts,
    })
}
