use std::collections::HashMap;

use runinator_models::errors::SendableError;
use runinator_models::runs::TaskExecutionResult;
use runinator_plugin::cancel::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{Shape, check_cancelled, open};
use crate::engine::Engine;
use crate::errors::INVALID_ARGUMENT;
use crate::export::{ExportSpec, export_rows};
use crate::helpers::{build_runtime, normalize_timeout, to_sendable};
use crate::rowset::StepOutcome;
use crate::statement::{StatementInput, StatementSpec};

#[derive(Deserialize)]
pub struct ScriptRequest {
    pub engine: Engine,
    pub connection: String,
    pub statements: Vec<StatementInput>,
    /// run every statement inside one transaction, rolling back on the first failure.
    #[serde(default)]
    pub transaction: bool,
    #[serde(default)]
    pub shape: Shape,
    /// when set, every row-returning step is also written to its own file.
    #[serde(default)]
    pub export: Option<ExportSpec>,
}

/// run an ordered list of statements. steps that return rows are reported as rows, the rest as
/// affected counts, so one action covers migrations, batches, and multi-query reports.
pub fn run(
    parameters: Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let request: ScriptRequest = serde_json::from_value(parameters).map_err(to_sendable)?;
    if request.statements.is_empty() {
        return Err(INVALID_ARGUMENT.error("'statements' must contain at least one statement"));
    }

    let timeout = normalize_timeout(timeout_secs);
    let statements = request
        .statements
        .into_iter()
        .map(|input| StatementSpec::resolve(input.into_fields(), request.engine))
        .collect::<Result<Vec<_>, SendableError>>()?;

    check_cancelled(&token)?;
    let runtime = std::sync::Arc::new(build_runtime()?);
    let connector = open(request.engine, &request.connection, runtime)?;
    let outcomes = connector.script(&statements, request.transaction, timeout)?;
    check_cancelled(&token)?;

    let mut counts = HashMap::new();
    let mut artifacts = Vec::new();
    let mut exports = Vec::new();
    let mut steps = Vec::with_capacity(outcomes.len());
    let mut total_rows = 0usize;
    let mut total_affected = 0u64;

    for (index, outcome) in outcomes.iter().enumerate() {
        let name = statements
            .get(index)
            .and_then(|statement| statement.name())
            .map(str::to_string)
            .unwrap_or_else(|| format!("statement_{:02}", index + 1));

        match outcome {
            StepOutcome::Rows(rows) => {
                total_rows += rows.row_count();
                let mut step = match request.shape.project(rows) {
                    Value::Object(map) => map,
                    other => {
                        let mut map = serde_json::Map::new();
                        map.insert("result".to_string(), other);
                        map
                    }
                };
                step.insert("name".to_string(), json!(name));
                step.insert("kind".to_string(), json!("rows"));

                if let Some(spec) = &request.export {
                    let exported = export_rows(rows, spec, &name, index, &mut counts)?;
                    step.insert("export".to_string(), exported.to_json());
                    exports.push(exported.to_json());
                    artifacts.push(exported.to_artifact());
                }

                steps.push(Value::Object(step));
            }
            StepOutcome::Affected(exec) => {
                total_affected += exec.rows_affected;
                steps.push(json!({
                    "name": name,
                    "kind": "affected",
                    "rows_affected": exec.rows_affected,
                    "last_insert_id": exec.last_insert_id.clone().unwrap_or(Value::Null),
                }));
            }
        }
    }

    let output = json!({
        "provider": "db",
        "engine": request.engine.as_str(),
        "shape": request.shape.as_str(),
        "transaction": request.transaction,
        "steps": steps,
        "step_count": outcomes.len(),
        "row_count": total_rows,
        "rows_affected": total_affected,
        "exports": exports,
    });

    Ok(TaskExecutionResult {
        message: Some(format!(
            "Ran {} statement(s): {total_rows} row(s) returned, {total_affected} row(s) affected",
            outcomes.len()
        )),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts,
    })
}
