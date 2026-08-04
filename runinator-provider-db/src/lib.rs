//! a generic database provider: sqlite, postgres, mysql, and mongodb behind one action surface.
//! statements are the primitive — rows come back typed, non-queries come back as counts, and
//! spreadsheet export is an option on a query rather than the reason to run one.

mod actions;
mod connector;
mod engine;
mod errors;
mod export;
mod helpers;
mod rowset;
mod statement;

use std::sync::Arc;

use runinator_models::{
    errors::SendableError,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    runs::{ProviderExecutionRequest, TaskExecutionResult},
    types::RuninatorField,
};
use runinator_plugin::provider::{Provider, ProviderEventSink};
use serde_json::json;

#[derive(Clone)]
pub struct DbProvider;

/// the engine selector shared by every action.
fn engine_parameter() -> ParameterMetadata {
    // mongodb is only advertised when this build actually carries the driver, so a pack that
    // names it fails wdl type-checking rather than at run time.
    let mut engines = vec![
        json!("sqlite").into(),
        json!("postgres").into(),
        json!("mysql").into(),
    ];
    if cfg!(feature = "mongo") {
        engines.push(json!("mongodb").into());
    }

    ParameterMetadata::required("engine", RuninatorType::Enum(engines))
        .with_description("Database engine to connect to")
}

/// the connection string, always secret so authors reference it as `secret.*` and the worker
/// resolves it late.
fn connection_parameter() -> ParameterMetadata {
    ParameterMetadata::required("connection", RuninatorType::String)
        .with_description("Connection string or URI; include the database name")
        .secret()
}

fn statement_parameters() -> Vec<ParameterMetadata> {
    vec![
        ParameterMetadata::optional("name", RuninatorType::String)
            .with_description("Label for this statement, used in results and export filenames"),
        ParameterMetadata::optional("sql", RuninatorType::String)
            .with_description("Statement text for sqlite, postgres, and mysql"),
        ParameterMetadata::optional("params", RuninatorType::array(RuninatorType::Any))
            .with_description("Positional bind parameters ($1.. on postgres, ? elsewhere)"),
        ParameterMetadata::optional("collection", RuninatorType::String)
            .with_description("Target collection for mongodb"),
        ParameterMetadata::optional("find", RuninatorType::Any)
            .with_description("mongodb filter document for a find"),
        ParameterMetadata::optional("aggregate", RuninatorType::array(RuninatorType::Any))
            .with_description("mongodb aggregation pipeline"),
        ParameterMetadata::optional("insert", RuninatorType::array(RuninatorType::Any))
            .with_description("mongodb documents to insert"),
        ParameterMetadata::optional("update", RuninatorType::Any)
            .with_description("mongodb update as { filter, set } or { filter, update }"),
        ParameterMetadata::optional("delete", RuninatorType::Any)
            .with_description("mongodb filter document for a delete"),
        ParameterMetadata::optional("command", RuninatorType::Any)
            .with_description("Raw mongodb command passed to runCommand"),
        ParameterMetadata::optional("options", RuninatorType::Any)
            .with_description("projection, sort, limit, skip, upsert, and multi for mongodb"),
    ]
}

fn shape_parameter() -> ParameterMetadata {
    ParameterMetadata::optional(
        "shape",
        RuninatorType::Enum(vec![json!("rows").into(), json!("table").into()]),
    )
    .with_description("'rows' returns typed objects; 'table' returns flat string headers and rows")
    .with_default(json!("rows"))
}

fn export_parameter() -> ParameterMetadata {
    ParameterMetadata::optional(
        "export",
        RuninatorType::typed_structure([
            ("folder", RuninatorField::required(RuninatorType::String)),
            ("format", RuninatorField::optional(RuninatorType::String)),
            ("name", RuninatorField::optional(RuninatorType::String)),
            (
                "file_prefix",
                RuninatorField::optional(RuninatorType::String),
            ),
        ]),
    )
    .with_description("Also write the rows to an Excel or CSV file and attach it as an artifact")
}

fn column_type() -> RuninatorType {
    RuninatorType::structure([
        ("name", RuninatorType::String),
        ("type", RuninatorType::String),
        ("native_type", RuninatorType::String),
    ])
}

fn export_result_type() -> RuninatorType {
    RuninatorType::array(RuninatorType::structure([
        ("name", RuninatorType::String),
        ("rows", RuninatorType::Integer),
        ("path", RuninatorType::String),
        ("format", RuninatorType::String),
        ("size_bytes", RuninatorType::Integer),
    ]))
}

fn query_action() -> ActionMetadata {
    let mut parameters = vec![engine_parameter(), connection_parameter()];
    parameters.extend(statement_parameters());
    parameters.push(shape_parameter());
    parameters.push(export_parameter());

    ActionMetadata::new(
        "query",
        "Run a row-returning statement and return the rows, optionally exporting them",
    )
    .with_parameters(parameters)
    .with_results(vec![
        ResultMetadata::new("provider", RuninatorType::String),
        ResultMetadata::new("engine", RuninatorType::String),
        ResultMetadata::new("shape", RuninatorType::String),
        ResultMetadata::new("columns", RuninatorType::array(column_type())),
        ResultMetadata::new("rows", RuninatorType::array(RuninatorType::Any)),
        ResultMetadata::new("headers", RuninatorType::array(RuninatorType::String)),
        ResultMetadata::new("row_count", RuninatorType::Integer),
        ResultMetadata::new("exports", export_result_type()),
    ])
}

fn execute_action() -> ActionMetadata {
    let mut parameters = vec![engine_parameter(), connection_parameter()];
    parameters.extend(statement_parameters());

    ActionMetadata::new(
        "execute",
        "Run a non-query statement (insert, update, delete, or DDL) and return the affected count",
    )
    .with_parameters(parameters)
    .with_results(vec![
        ResultMetadata::new("provider", RuninatorType::String),
        ResultMetadata::new("engine", RuninatorType::String),
        ResultMetadata::new("rows_affected", RuninatorType::Integer),
        ResultMetadata::new("last_insert_id", RuninatorType::Any),
    ])
}

fn script_action() -> ActionMetadata {
    ActionMetadata::new(
        "script",
        "Run an ordered list of statements, optionally in a single transaction",
    )
    .with_parameters(vec![
        engine_parameter(),
        connection_parameter(),
        ParameterMetadata::required("statements", RuninatorType::array(RuninatorType::Any))
            .with_description("Statement texts or statement objects, run in order"),
        ParameterMetadata::optional("transaction", RuninatorType::Boolean)
            .with_description("Wrap every statement in one transaction and roll back on failure")
            .with_default(json!(false)),
        shape_parameter(),
        export_parameter(),
    ])
    .with_results(vec![
        ResultMetadata::new("provider", RuninatorType::String),
        ResultMetadata::new("engine", RuninatorType::String),
        ResultMetadata::new("shape", RuninatorType::String),
        ResultMetadata::new("transaction", RuninatorType::Boolean),
        ResultMetadata::new("steps", RuninatorType::array(RuninatorType::Any)),
        ResultMetadata::new("step_count", RuninatorType::Integer),
        ResultMetadata::new("row_count", RuninatorType::Integer),
        ResultMetadata::new("rows_affected", RuninatorType::Integer),
        ResultMetadata::new("exports", export_result_type()),
    ])
}

fn provision_action() -> ActionMetadata {
    ActionMetadata::new(
        "provision",
        "Ensure the database exists, apply schema statements, then seed rows",
    )
    .with_parameters(vec![
        engine_parameter(),
        connection_parameter(),
        ParameterMetadata::optional("admin_connection", RuninatorType::String)
            .with_description(
                "Maintenance connection used to CREATE DATABASE on postgres and mysql",
            )
            .secret(),
        ParameterMetadata::optional("database", RuninatorType::String)
            .with_description("Database name to create; derived from the connection when omitted"),
        ParameterMetadata::optional("schema", RuninatorType::array(RuninatorType::Any))
            .with_description("DDL statements applied in order once the database exists"),
        ParameterMetadata::optional(
            "collections",
            RuninatorType::array(RuninatorType::typed_structure([
                ("name", RuninatorField::required(RuninatorType::String)),
                (
                    "indexes",
                    RuninatorField::optional(RuninatorType::array(RuninatorType::Any)),
                ),
            ])),
        )
        .with_description("mongodb collections and indexes to create"),
        ParameterMetadata::optional(
            "seed",
            RuninatorType::array(RuninatorType::typed_structure([
                ("table", RuninatorField::optional(RuninatorType::String)),
                (
                    "collection",
                    RuninatorField::optional(RuninatorType::String),
                ),
                (
                    "rows",
                    RuninatorField::required(RuninatorType::array(RuninatorType::Any)),
                ),
                (
                    "on_conflict",
                    RuninatorField::optional(RuninatorType::String),
                ),
            ])),
        )
        .with_description("Rows inserted after the schema is in place"),
    ])
    .with_results(vec![
        ResultMetadata::new("provider", RuninatorType::String),
        ResultMetadata::new("engine", RuninatorType::String),
        ResultMetadata::new("created", RuninatorType::Boolean),
        ResultMetadata::new("applied", RuninatorType::Integer),
        ResultMetadata::new("seeded", RuninatorType::Integer),
    ])
}

fn inspect_action() -> ActionMetadata {
    ActionMetadata::new("inspect", "List tables or collections and their columns")
        .with_parameters(vec![engine_parameter(), connection_parameter()])
        .with_results(vec![
            ResultMetadata::new("provider", RuninatorType::String),
            ResultMetadata::new("engine", RuninatorType::String),
            ResultMetadata::new(
                "tables",
                RuninatorType::array(RuninatorType::structure([
                    ("name", RuninatorType::String),
                    ("schema", RuninatorType::String),
                    ("columns", RuninatorType::array(RuninatorType::Any)),
                ])),
            ),
            ResultMetadata::new("table_count", RuninatorType::Integer),
        ])
}

impl Provider for DbProvider {
    fn name(&self) -> String {
        "db".to_string()
    }

    fn metadata(&self) -> ProviderMetadata {
        ProviderMetadata {
            name: self.name(),
            actions: vec![
                query_action(),
                execute_action(),
                script_action(),
                provision_action(),
                inspect_action(),
            ],
            metadata: ProviderRuntimeMetadata {
                credential_scopes: vec!["db".into()],
                contract: None,
            },
        }
    }

    fn execute_service(
        &self,
        request: ProviderExecutionRequest,
        _sink: Option<Arc<dyn ProviderEventSink>>,
        token: runinator_plugin::cancel::CancellationToken,
    ) -> Result<TaskExecutionResult, SendableError> {
        let parameters = request.parameters.into();
        let timeout = request.timeout_secs;

        match request.action_function.as_str() {
            "query" => actions::query::run(parameters, timeout, token),
            "execute" => actions::execute::run(parameters, timeout, token),
            "script" => actions::script::run(parameters, timeout, token),
            "provision" => actions::provision::run(parameters, timeout, token),
            "inspect" => actions::inspect::run(parameters, timeout, token),
            _ => Err(errors::UNSUPPORTED_CALL.error(format!(
                "Unsupported database provider call '{}'",
                request.action_function
            ))),
        }
    }
}

#[cfg(test)]
mod tests;
