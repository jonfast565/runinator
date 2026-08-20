//! convert stored `std.run`/`std.exec` definitions into `invocation` definitions.
//!
//! this is the one-way half of the cutover. it is deliberately *not* run automatically at startup,
//! unlike the two idempotent backfills beside it (`migrate_workflow_execution_states`,
//! `reconcile_legacy_mutexes`), because it is neither idempotent in the way those are nor safe to
//! run against a live system: a run that is mid-flight against the old node shape would find its
//! node replaced underneath it.
//!
//! so the entry point is [`convert_definitions`], called by an operator through the control plane
//! once the preflight in [`preflight`] passes. the two are separate on purpose — the dry run must be
//! able to answer "would every definition convert?" without writing anything, because the answer
//! "all but one" has to be knowable *before* the other several hundred have been rewritten.
//!
//! in-flight runs need no conversion and must not get one: `workflow_runs.workflow_snapshot` already
//! pins each run to the definition it started with, which is what makes draining a precondition
//! rather than a data-migration problem.

use runinator_compute::{CallableCatalog, assemble_module, parse_program};
use runinator_database::interfaces::prelude::*;
use runinator_models::errors::SendableError;
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowDefinition;

/// what a dry run found.
#[derive(Debug, Default, PartialEq)]
pub struct MigrationReport {
    /// definitions holding at least one convertible node.
    pub convertible: Vec<String>,
    /// definitions already converted, or with nothing to convert.
    pub unchanged: Vec<String>,
    /// definitions that would fail, with the reason. any entry here aborts the migration.
    pub blocked: Vec<(String, String)>,
}

impl MigrationReport {
    /// whether the migration may proceed.
    ///
    /// one blocked definition stops all of them. a partial conversion would leave the fleet running
    /// two node shapes with no record of which definitions are in which state, and the operator has
    /// no way to tell from the outside — so the failure is reported whole and nothing is written.
    pub fn is_clear(&self) -> bool {
        self.blocked.is_empty()
    }
}

/// read every definition and report what a conversion would do, writing nothing.
pub async fn preflight<T: DatabaseImpl>(db: &T) -> Result<MigrationReport, SendableError> {
    let workflows = db.fetch_workflows().await?;
    let mut report = MigrationReport::default();
    for workflow in workflows {
        let name = workflow.name.clone();
        match convert_definition(&workflow) {
            Ok(Some(_)) => report.convertible.push(name),
            Ok(None) => report.unchanged.push(name),
            Err(err) => report.blocked.push((name, err.to_string())),
        }
    }
    Ok(report)
}

/// convert every definition, or none.
///
/// runs the dry run first and refuses on any blocked definition, so a partially converted fleet is
/// not a state this can produce.
pub async fn convert_definitions<T: DatabaseImpl>(
    db: &T,
) -> Result<MigrationReport, SendableError> {
    let report = preflight(db).await?;
    if !report.is_clear() {
        return Ok(report);
    }
    for workflow in db.fetch_workflows().await? {
        let Some(converted) = convert_definition(&workflow)? else {
            continue;
        };
        db.upsert_workflow(&converted).await?;
    }
    Ok(report)
}

/// convert one definition, or `None` when it holds nothing to convert.
///
/// public so the revision-restore path can run an old revision through it: `restore_workflow_revision`
/// already re-validates an old revision against today's catalog and saves the result as a *new*
/// revision, which is exactly the seam a converter belongs in. historical revision bytes are left
/// untouched — they are append-only, and rewriting them would destroy the record of what actually
/// ran.
pub fn convert_definition(
    workflow: &WorkflowDefinition,
) -> Result<Option<WorkflowDefinition>, SendableError> {
    let graph: Value = serde_json::to_value(&workflow.definition)?.into();
    let Some(nodes) = graph.get("nodes").and_then(Value::as_array) else {
        return Ok(None);
    };
    let nodes = nodes.clone();
    let functions = function_entries(&graph)?;
    let catalog = catalog_for(&functions);

    let mut converted = Vec::with_capacity(nodes.len());
    let mut changed = false;
    for node in &nodes {
        match convert_node(node, &functions, &catalog)? {
            Some(node) => {
                converted.push(node);
                changed = true;
            }
            None => converted.push(node.clone()),
        }
    }
    if !changed {
        return Ok(None);
    }

    let mut graph = graph;
    if let Value::Object(object) = &mut graph {
        object.insert("nodes".into(), Value::Array(converted));
        // the executable function bodies move into each invocation's module. the render-only
        // signature hints under `metadata.rexrap.functions` stay: decompile reads those, and they were
        // never what the engine called.
        if let Some(Value::Object(metadata)) = object.get_mut("metadata") {
            metadata.remove("functions");
        }
    }
    let mut out = workflow.clone();
    out.definition = runinator_models::workflows::WorkflowGraph::from_value(graph)
        .map_err(|err| -> SendableError { err.into() })?;
    Ok(Some(out))
}

/// convert one node, or `None` when it is not a legacy compute node.
fn convert_node(
    node: &Value,
    functions: &[AssembledFunction],
    catalog: &CallableCatalog,
) -> Result<Option<Value>, SendableError> {
    if node.get("kind").and_then(Value::as_str) != Some("action") {
        return Ok(None);
    }
    let Some(action) = node.get("action") else {
        return Ok(None);
    };
    if action.get("provider").and_then(Value::as_str) != Some("std") {
        return Ok(None);
    }
    // `std.code` stays an action: it is a container invocation with no program to assemble, and the
    // plan's "one-call invocation" framing would buy nothing — the node already dispatches exactly
    // once and settles.
    if !matches!(
        action.get("function").and_then(Value::as_str),
        Some("run") | Some("exec")
    ) {
        return Ok(None);
    }
    let Some(program) = action
        .get("configuration")
        .and_then(|configuration| configuration.get("program"))
        .and_then(Value::as_array)
    else {
        return Ok(None);
    };

    let parsed = parse_program(&Value::Array(program.clone()))
        .map_err(|err| -> SendableError { Box::new(err) })?;
    let module = assemble_module(&parsed, functions, catalog)
        .map_err(|err| -> SendableError { Box::new(err) })?;

    let mut parameters = Map::new();
    parameters.insert("module".into(), serde_json::to_value(&module)?.into());
    // the statement tree is retained exactly as the compiler would have retained it, so a converted
    // definition decompiles to the same rexrap a freshly compiled one does.
    parameters.insert("source".into(), Value::Array(program.clone()));
    if let Some(timeout) = action.get("timeout_seconds").and_then(Value::as_i64) {
        parameters.insert("timeout_seconds".into(), Value::from(timeout));
    }

    let Value::Object(fields) = node else {
        return Ok(None);
    };
    let mut out = fields.clone();
    out.insert("kind".into(), Value::String("invocation".into()));
    out.insert("parameters".into(), Value::Object(parameters));
    out.remove("action");
    Ok(Some(Value::Object(out)))
}

/// a user function in the shape the assembler takes.
type AssembledFunction = (
    String,
    Vec<String>,
    runinator_models::workflow_ast::ComputeProgram,
    Option<u32>,
);

/// read `metadata.functions` into assembler input.
fn function_entries(graph: &Value) -> Result<Vec<AssembledFunction>, SendableError> {
    let Some(entries) = graph
        .get("metadata")
        .and_then(|metadata| metadata.get("functions"))
        .and_then(Value::as_array)
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| -> SendableError { "a stored function has no name".into() })?
            .to_string();
        let params = entry
            .get("params")
            .and_then(Value::as_array)
            .map(|params| {
                params
                    .iter()
                    .filter_map(|param| param.get("name").and_then(Value::as_str))
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // an expression body is a one-statement `return`, which is what it means.
        let program = match (entry.get("program"), entry.get("body")) {
            (Some(program), _) => program.clone(),
            (None, Some(body)) => Value::Array(vec![Value::Object(Map::from_iter([(
                "$return".into(),
                body.clone(),
            )]))]),
            (None, None) => {
                return Err(format!("stored function '{name}' has no body").into());
            }
        };
        let body = parse_program(&program).map_err(|err| -> SendableError { Box::new(err) })?;
        let max_depth = entry
            .get("recursive")
            .and_then(|recursive| recursive.get("max_depth"))
            .and_then(Value::as_i64)
            .map(|depth| depth as u32);
        out.push((name, params, body, max_depth));
    }
    Ok(out)
}

/// the catalog a stored definition's calls are classified against.
///
/// provider metadata is deliberately absent: a stored program's calls were already resolved by the
/// compiler that produced it, and a name the catalog does not recognize falls through to `Local`,
/// which the vm reports by name if it is genuinely unknown. re-deriving provider classification
/// here from *today's* registered providers would make the conversion depend on which workers
/// happened to be connected.
fn catalog_for(functions: &[AssembledFunction]) -> CallableCatalog {
    let mut catalog = CallableCatalog::builtin();
    for (name, params, _, _) in functions {
        catalog.add_local(
            name.clone(),
            params.len(),
            runinator_models::invocation::EffectClass::Pure,
        );
    }
    catalog
}

#[cfg(test)]
#[path = "invocation_migration_tests.rs"]
mod tests;
