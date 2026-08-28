use runinator_models::errors::SendableError;
use runinator_models::runs::TaskExecutionResult;
use runinator_plugin::cancel::CancellationToken;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::actions::{check_cancelled, open};
use crate::connector::{ProvisionSpec, SeedSpec};
use crate::engine::Engine;
use crate::helpers::{build_runtime, normalize_timeout, to_sendable};
use crate::statement::{StatementInput, StatementSpec};

#[derive(Deserialize)]
pub struct ProvisionRequest {
    pub engine: Engine,
    pub connection: String,
    /// maintenance connection used to issue `CREATE DATABASE` on postgres and mariadb.
    #[serde(default)]
    pub admin_connection: Option<String>,
    /// database name to create; derived from `connection` when omitted.
    #[serde(default)]
    pub database: Option<String>,
    /// ddl applied after the database exists, in order, inside one transaction where supported.
    #[serde(default)]
    pub schema: Vec<StatementInput>,
    /// rows inserted after the schema is in place.
    #[serde(default)]
    pub seed: Vec<SeedSpec>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// ensure the database exists, apply schema, then seed. each phase is optional, so this covers
/// "just create the file" as well as a full fixture build.
pub fn run(
    parameters: Value,
    timeout_secs: i64,
    token: CancellationToken,
) -> Result<TaskExecutionResult, SendableError> {
    let request: ProvisionRequest = serde_json::from_value(parameters).map_err(to_sendable)?;
    if let Some(name) = request.extra.keys().next() {
        return Err(
            crate::errors::INVALID_ARGUMENT.error(format!("unknown provision field '{name}'"))
        );
    }
    let timeout = normalize_timeout(timeout_secs);

    let schema = request
        .schema
        .into_iter()
        .map(|input| StatementSpec::resolve(input.into_fields(), request.engine))
        .collect::<Result<Vec<_>, SendableError>>()?;

    check_cancelled(&token)?;
    let runtime = std::sync::Arc::new(build_runtime()?);
    let connector = open(request.engine, &request.connection, runtime)?;

    let spec = ProvisionSpec {
        admin_connection: request.admin_connection,
        database: request.database,
    };
    let created = connector.ensure_database(&spec, timeout)?;
    check_cancelled(&token)?;

    let applied = if schema.is_empty() {
        0
    } else {
        connector.script(&schema, true, timeout)?.len()
    };
    check_cancelled(&token)?;

    let seeded = if request.seed.is_empty() {
        0
    } else {
        connector.seed(&request.seed, timeout)?
    };

    let output = json!({
        "provider": "db",
        "engine": request.engine.as_str(),
        "created": created,
        "applied": applied,
        "seeded": seeded,
    });

    let created_label = if created {
        "created"
    } else {
        "already present"
    };
    Ok(TaskExecutionResult {
        message: Some(format!(
            "Database {created_label}; applied {applied} schema statement(s), seeded {seeded} row(s)"
        )),
        output_json: Some(output.into()),
        chunks: Vec::new(),
        artifacts: Vec::new(),
    })
}
use std::collections::BTreeMap;
