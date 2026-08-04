use runinator_models::errors::SendableError;
use runinator_models::runs::TaskExecutionResult;
use runinator_plugin::cancel::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{check_cancelled, open};
use crate::engine::Engine;
use crate::helpers::{build_runtime, normalize_timeout, to_sendable};

#[derive(Deserialize)]
pub struct InspectRequest {
    pub engine: Engine,
    pub connection: String,
}

/// list the tables (or collections) and their columns, so a workflow can branch on what exists
/// without hand-writing an `information_schema` query per engine.
pub fn run(
    parameters: Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let request: InspectRequest = serde_json::from_value(parameters).map_err(to_sendable)?;
    let timeout = normalize_timeout(timeout_secs);

    check_cancelled(&token)?;
    let runtime = std::sync::Arc::new(build_runtime()?);
    let connector = open(request.engine, &request.connection, runtime)?;
    let tables = connector.inspect(timeout)?;

    let output = json!({
        "provider": "db",
        "engine": request.engine.as_str(),
        "tables": tables,
        "table_count": tables.len(),
    });

    Ok(TaskExecutionResult {
        message: Some(format!("Found {} table(s)", tables.len())),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
