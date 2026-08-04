use runinator_models::errors::SendableError;
use runinator_models::runs::TaskExecutionResult;
use runinator_plugin::cancel::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{check_cancelled, open};
use crate::engine::Engine;
use crate::helpers::{build_runtime, normalize_timeout, to_sendable};
use crate::statement::{StatementFields, StatementSpec};

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub engine: Engine,
    pub connection: String,
    #[serde(flatten)]
    pub statement: StatementFields,
}

/// run one statement for its effect. the result is a count, not a row set.
pub fn run(
    parameters: Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let request: ExecuteRequest = serde_json::from_value(parameters).map_err(to_sendable)?;
    let timeout = normalize_timeout(timeout_secs);
    let statement = StatementSpec::resolve(request.statement, request.engine)?;

    check_cancelled(&token)?;
    let runtime = std::sync::Arc::new(build_runtime()?);
    let connector = open(request.engine, &request.connection, runtime)?;
    let outcome = connector.execute(&statement, timeout)?;

    let mut output = match outcome.to_json() {
        Value::Object(map) => map,
        other => {
            let mut map = serde_json::Map::new();
            map.insert("result".to_string(), other);
            map
        }
    };
    output.insert("provider".to_string(), json!("db"));
    output.insert("engine".to_string(), json!(request.engine.as_str()));
    let output = Value::Object(output);

    Ok(TaskExecutionResult {
        message: Some(format!("Affected {} row(s)", outcome.rows_affected)),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
