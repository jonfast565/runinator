// lowers the rexrap ast into the existing runinator json workflow model. sequential
// statements imply forward edges; control blocks expand into the matching control nodes.
// the output is a WorkflowDefinition whose `definition` is `{ start, nodes: [...] }`.

mod blocks;
mod compute_block;
mod expr;
mod inline;
mod spreads;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use runinator_models::providers::{ActionMetadata, ProviderMetadata};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::{WorkflowDefinition, WorkflowGraph};

use runinator_rexrap_sema::CompileOptions;
use runinator_rexrap_sema::desugar::AliasTable;
use runinator_rexrap_sema::types;
use runinator_rexrap_syntax::ast::*;
use runinator_rexrap_syntax::errors::{RexRapError, Span};

/// a binding from a loop/map variable to the node output it reads from.
#[derive(Clone)]
struct VarBinding {
    name: String,
    node_id: String,
    base: Vec<PathSeg>,
}

/// The runtime target represented by an authored `task[T]` binding. Detached subflows and
/// provider tasks both produce awaitable handles, but their durable identifiers are different.
#[derive(Clone)]
enum TaskBinding {
    Subflow,
    Provider,
}

struct Lowerer {
    nodes: Vec<Value>,
    used_ids: HashSet<String>,
    /// the source span of the statement currently being lowered, so every node it produces can be
    /// traced back to the text that produced it. see [`NodeSpan`].
    current_span: Option<Span>,
    /// node id -> the span of the statement that produced it, in emission order.
    spans: Vec<NodeSpan>,
    task_bindings: HashMap<String, TaskBinding>,
    /// `task fn` definitions, inlined at each call site rather than compiled to a callable.
    task_fns: HashMap<String, FunctionDef>,
    /// handles explicitly dropped by `detach`, so an unconsumed one can be reported as an error.
    detached: HashSet<String>,
    counter: u64,
    /// a counter of its own for `resume` node ids — see `fresh_resume`.
    resume_counter: u64,
    start_id: String,
    end_id: String,
    fail_id: String,
    scope: Vec<VarBinding>,
    // declared `let <id>: <type>` annotations, kept for graph metadata so decompile can
    // re-emit the authored surface form.
    declared_types: Vec<(String, Value)>,
    // machine-readable declared node output types consumed by backend validation.
    declared_type_hints: Vec<(String, Value)>,
    // header alias declarations, used to expand `...alias` spreads while lowering.
    aliases: AliasTable,
    // per-node `...alias` spread recipes (node id -> recipe segments), kept for graph metadata so
    // decompile can resugar the spreads. empty for spread-free workflows.
    spreads: Map,
    // control-block ids that were explicitly authored with `@id`, kept so terse decompile can
    // preserve them without surfacing every generated control id.
    control_ids: Vec<String>,
    // authored item/index names for loop-like controls, keyed by lowered node id.
    control_vars: Map,
    // parallel branch labels and their generated stop nodes, kept so decompile can recover a
    // selected join without making labels part of the runtime graph contract.
    parallel_branches: Map,
    // in-scope local names (compute-block `let`s and lambda params), so a bare local path lowers to
    // a `let` ref. interior-mutable because `lower_expr` (`&self`) scopes a lambda's params while
    // lowering its body, whether the lambda sits in a compute block or inline in any expression.
    compute_locals: std::cell::RefCell<HashSet<String>>,
    // published packaged-function exports a `functions.<pkg>.<export>(...)` call may bind to.
    functions: Vec<runinator_models::functions::FunctionCatalogEntry>,
    // resolved `type <Name>` declarations, consulted when lowering named type references.
    named_types: std::collections::BTreeMap<String, runinator_models::types::RuninatorType>,
    // user `fn` definitions in their lowered metadata form, available while the body is lowered so
    // an `invocation` node can assemble them into its module.
    lowered_functions: Vec<Value>,
    // emit `invocation` nodes carrying compiled bytecode instead of `std.run`/`std.exec` action
    // base directory used for compile-time `file("...")` text includes.
    source_dir: Option<PathBuf>,
    // the callable registry (intrinsics + user functions), used to resolve keyword arguments.
    registry: runinator_rexrap_sema::registry::FunctionRegistry,
    // provider actions available at compile time, used to generate action output type hints.
    provider_actions: std::collections::HashMap<(String, String), ActionMetadata>,
    // the same provider metadata, kept whole so the assembler's catalog can classify a called name
    // as a provider action rather than re-deriving it from the flattened action map.
    provider_metadata: Vec<runinator_models::providers::ProviderMetadata>,
}

struct LowerEntry {
    entry: String,
    collector: Option<String>,
}

/// One graph node paired with the source span of the statement that produced it.
///
/// Spans index the text the document was parsed from, so they are only meaningful alongside that
/// exact text — see `decompile_with_spans`, which returns both together.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeSpan {
    pub node_id: String,
    pub start: usize,
    pub end: usize,
}

pub fn lower_document(
    document: &Document,
    options: &CompileOptions,
) -> Result<Vec<WorkflowDefinition>, RexRapError> {
    document
        .workflows
        .iter()
        .map(|workflow| {
            lower_workflow(document, workflow, options).map(|(definition, _)| definition)
        })
        .collect()
}

/// Lower every workflow in `document`, keeping the node-to-span map for each.
pub fn lower_document_with_spans(
    document: &Document,
    options: &CompileOptions,
) -> Result<Vec<(WorkflowDefinition, Vec<NodeSpan>)>, RexRapError> {
    document
        .workflows
        .iter()
        .map(|workflow| lower_workflow(document, workflow, options))
        .collect()
}

fn lower_workflow(
    document: &Document,
    workflow: &Workflow,
    options: &CompileOptions,
) -> Result<(WorkflowDefinition, Vec<NodeSpan>), RexRapError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    // the callable registry resolves keyword args in both the workflow body and function bodies.
    lowerer.registry =
        runinator_rexrap_sema::registry::FunctionRegistry::build(&document.functions);
    lowerer.provider_metadata = options.all_providers();
    lowerer.provider_actions = provider_actions(&lowerer.provider_metadata);
    lowerer.functions = options.functions.clone();
    // collect the header aliases so spreads can be expanded (graph) and recorded (sidecar) while
    // lowering, where node ids are available to key the recipes.
    lowerer.aliases = runinator_rexrap_sema::desugar::collect_aliases(&workflow.aliases)?;
    // resolve named `type <Name>` declarations so they can be referenced by parameter/let types.
    lowerer.resolve_type_decls(&workflow.type_decls)?;
    // user `fn` definitions are lowered *before* the body, not after, because an `invocation` node
    // assembles them into its module as it is emitted. the lowered form is identical either way —
    // function bodies do not depend on the body's node ids — so this only moves when it happens.
    lowerer.lowered_functions = lowerer.lower_functions(&document.functions)?;
    // `task fn`s are inlined, not compiled into the function table, so keep them by name.
    for def in &document.functions {
        if def.is_task {
            lowerer.task_fns.insert(def.name.clone(), def.clone());
        }
    }
    let end_id = lowerer.end_id.clone();
    // handler regions lower into the same node list as the main flow, just unreachable from `start`.
    // they go *first* because decompile emits them first, in the header: generated node ids come
    // from a running counter, so the two orders have to agree or a region's `resume` node comes back
    // numbered differently and the round trip diverges on nothing but its id.
    let interrupts = lowerer.lower_interrupts(&workflow.interrupts)?;
    let body_entry = lowerer.lower_block(&workflow.body, &end_id)?;
    // named continuations lower after the main flow: they are reachable only by `continue <name>`.
    let joins = lowerer.lower_joins(&workflow.joins, &end_id)?;

    // the entry is an explicit `start -> <target>` when present, else the first statement.
    let entry = match &workflow.start {
        Some(target) => lowerer.target_id(target),
        None => body_entry,
    };

    // build the start node pointing at the entry, then append the terminals.
    let start_node = node(
        &lowerer.start_id,
        "start",
        vec![("transitions", transitions_next(&entry))],
    );
    let mut nodes = Vec::with_capacity(lowerer.nodes.len() + 3);
    nodes.push(start_node);
    nodes.append(&mut lowerer.nodes);
    nodes.push(node(&lowerer.end_id, "end", vec![]));
    nodes.push(node(&lowerer.fail_id, "fail", vec![]));

    // the header alias declarations, encoded as recipe segments so decompile can re-emit them.
    let mut alias_meta = Vec::with_capacity(workflow.aliases.len());
    for alias in &workflow.aliases {
        let segs = lowerer.entry_segs(&alias.entries)?;
        let mut entry = Map::new();
        entry.insert("name".into(), Value::String(alias.name.clone()));
        entry.insert("segs".into(), Value::Array(segs));
        alias_meta.push(Value::Object(entry));
    }

    let mut definition = Map::new();
    definition.insert("start".into(), Value::String(lowerer.start_id.clone()));
    definition.insert("nodes".into(), Value::Array(nodes));
    // the `rexrap` sidecar carries source hints that let decompile reproduce the original source and
    // backend validation consume declared node output types.
    let mut rexrap = Map::new();
    // record which nodes front a `join <name>` region so decompile can restore the declaration
    // rather than rendering its pass-through entry as an ordinary statement.
    if !joins.is_empty() {
        rexrap.insert("joins".into(), Value::Array(joins));
    }
    if !lowerer.declared_types.is_empty() {
        let mut types_map = Map::new();
        for (id, value) in &lowerer.declared_types {
            types_map.insert(id.clone(), value.clone());
        }
        rexrap.insert("types".into(), Value::Object(types_map));
    }
    if !lowerer.declared_type_hints.is_empty() {
        let mut hints_map = Map::new();
        for (id, value) in &lowerer.declared_type_hints {
            hints_map.insert(id.clone(), value.clone());
        }
        rexrap.insert("type_hints".into(), Value::Object(hints_map));
    }
    // named `type <Name>` declarations, recorded as name-preserving surface strings so a
    // declaration that references another declared type keeps that name on decompile.
    if !workflow.type_decls.is_empty() {
        let mut decls = Map::new();
        for decl in &workflow.type_decls {
            // validate the declaration resolves before recording its surface form.
            lowerer.lower_named_type(&decl.ty)?;
            decls.insert(
                decl.name.clone(),
                Value::String(runinator_rexrap_syntax::format::format_type(&decl.ty)),
            );
        }
        rexrap.insert("type_decls".into(), Value::Object(decls));
    }
    if let Some(output) = &workflow.output {
        let ty = lowerer.lower_named_type(output)?;
        let value = Value::encode(&ty)
            .map_err(|err| RexRapError::lower(format!("failed to encode output type: {err}")))?;
        rexrap.insert("output_type".into(), value);
    }
    // surface-form overrides for top-level workflow parameter fields whose type references a
    // declared name, so `params { cart: Cart }` decompiles back to the name instead of the
    // expanded struct shape.
    if let Some(TypeExpr::Struct { fields, .. }) = &workflow.input {
        let mut overrides = Map::new();
        for field in fields {
            if type_expr_uses_declared_name(&field.ty, &lowerer.named_types) {
                overrides.insert(
                    field.name.clone(),
                    Value::String(runinator_rexrap_syntax::format::format_type(&field.ty)),
                );
            }
        }
        if !overrides.is_empty() {
            rexrap.insert("input_types".into(), Value::Object(overrides));
        }
    }
    if !alias_meta.is_empty() {
        rexrap.insert("aliases".into(), Value::Array(alias_meta));
    }
    if !lowerer.spreads.is_empty() {
        rexrap.insert("spreads".into(), Value::Object(lowerer.spreads.clone()));
    }
    if !lowerer.control_ids.is_empty() {
        rexrap.insert(
            "control_ids".into(),
            Value::Array(
                lowerer
                    .control_ids
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect(),
            ),
        );
    }
    if !lowerer.control_vars.is_empty() {
        rexrap.insert(
            "control_vars".into(),
            Value::Object(lowerer.control_vars.clone()),
        );
    }
    if !lowerer.parallel_branches.is_empty() {
        rexrap.insert(
            "parallel_branches".into(),
            Value::Object(lowerer.parallel_branches.clone()),
        );
    }
    // per-function surface signatures (`(params) -> ret`), recorded so decompile can reconstruct the
    // typed `fn` headers the runtime `functions` form does not carry. the runtime ignores this hint.
    if !document.functions.is_empty() {
        let mut sigs = Map::new();
        for def in &document.functions {
            sigs.insert(
                def.name.clone(),
                Value::String(runinator_rexrap_syntax::format::format_fn_signature(def)),
            );
        }
        rexrap.insert("functions".into(), Value::Object(sigs));
    }
    // header `trigger cron` declarations, carried as runtime data the web service materializes on
    // import (unlike the render-only `rexrap` sidecar).
    let triggers = lowerer.lower_triggers(&workflow.triggers)?;
    // header `notify on ...` policies, carried the same way triggers are: runtime data the web
    // service materializes as pack-managed notification policy rows on import.
    let notifications = lowerer.lower_notifications(&workflow.notifications)?;
    let watches = lowerer.lower_watches(&workflow.watches)?;
    // header `correlate key <expr>`, carried as a runtime expression the engine resolves and stamps
    // onto each run's correlation key so `await workflow ... key` joins can match.
    let correlation = match &workflow.correlation {
        Some(expr) => Some(lowerer.lower_expr(expr)?),
        None => None,
    };
    let functions = lowerer.lowered_functions.clone();
    let mut metadata = Map::new();
    if !rexrap.is_empty() {
        metadata.insert("rexrap".into(), Value::Object(rexrap));
    }
    if !triggers.is_empty() {
        metadata.insert("triggers".into(), Value::Array(triggers));
    }
    if !notifications.is_empty() {
        metadata.insert("notifications".into(), Value::Array(notifications));
    }
    // the concurrency cap is read straight off the definition by the trigger loop, so it versions
    // with the workflow rather than being materialized into a separate row.
    if let Some(concurrency) = &workflow.concurrency {
        let mut entry = Map::new();
        entry.insert(
            "max_concurrent_runs".into(),
            Value::from(concurrency.max_concurrent_runs),
        );
        entry.insert(
            "on_conflict".into(),
            Value::String(concurrency.on_conflict.keyword().into()),
        );
        metadata.insert("concurrency".into(), Value::Object(entry));
    }
    if !watches.is_empty() {
        metadata.insert("watches".into(), Value::Array(watches));
    }
    if !interrupts.is_empty() {
        metadata.insert("interrupts".into(), Value::Array(interrupts));
    }
    if let Some(correlation) = correlation {
        metadata.insert("correlation".into(), correlation);
    }
    if !functions.is_empty() {
        metadata.insert("functions".into(), Value::Array(functions));
    }
    if !metadata.is_empty() {
        definition.insert("metadata".into(), Value::Object(metadata));
    }
    let graph = WorkflowGraph::from_value(Value::Object(definition)).map_err(RexRapError::lower)?;

    let input_type = match &workflow.input {
        Some(type_expr) => lowerer.lower_input_type(type_expr)?,
        None => Default::default(),
    };

    Ok((
        WorkflowDefinition {
            id: None,
            name: workflow.name.clone(),
            namespace: workflow.namespace.clone(),
            // org is assigned by the web service at import time, not during compilation.
            org_id: None,
            version: workflow.version.unwrap_or(options.default_version),
            enabled: options.enabled,
            input_type,
            definition: graph,
            created_at: None,
            updated_at: None,
        },
        std::mem::take(&mut lowerer.spans),
    ))
}

pub fn lower_expression_fragment(
    expr: &Expr,
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    lowerer.lower_expr(expr)
}

pub fn lower_condition_fragment(
    cond: &Cond,
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    lowerer.lower_cond(cond)
}

pub fn lower_do_fragment(
    body: &[ComputeLine],
    options: &CompileOptions,
) -> Result<Value, RexRapError> {
    let mut lowerer = Lowerer::new();
    lowerer.source_dir = options.source_dir.clone();
    lowerer.lower_do_fragment(body)
}

impl Lowerer {
    fn new() -> Self {
        let mut used_ids = HashSet::new();
        used_ids.insert("start".to_string());
        used_ids.insert("end".to_string());
        used_ids.insert("fail".to_string());
        Self {
            nodes: Vec::new(),
            used_ids,
            task_bindings: HashMap::new(),
            task_fns: HashMap::new(),
            detached: HashSet::new(),
            current_span: None,
            spans: Vec::new(),
            counter: 0,
            resume_counter: 0,
            start_id: "start".to_string(),
            end_id: "end".to_string(),
            fail_id: "fail".to_string(),
            scope: Vec::new(),
            declared_types: Vec::new(),
            declared_type_hints: Vec::new(),
            aliases: AliasTable::new(),
            spreads: Map::new(),
            control_ids: Vec::new(),
            control_vars: Map::new(),
            parallel_branches: Map::new(),
            compute_locals: std::cell::RefCell::new(HashSet::new()),
            named_types: std::collections::BTreeMap::new(),
            lowered_functions: Vec::new(),
            source_dir: None,
            registry: runinator_rexrap_sema::registry::FunctionRegistry::build(&[]),
            provider_actions: std::collections::HashMap::new(),
            provider_metadata: Vec::new(),
            functions: Vec::new(),
        }
    }

    /// rewrite a `functions.<pkg>.<export>(...)` call into the action the runtime dispatches.
    ///
    /// the authored surface names a package and an export; the runtime has one provider,
    /// `functions`, with one action, `invoke`. the export is named by the binding this attaches,
    /// which pins the **exact version and artifact digest** resolved right now — that is what makes
    /// a later alias movement unable to reach into a workflow that already compiled.
    ///
    /// returns false for an ordinary action, which must be left completely untouched.
    fn apply_function_binding(
        &self,
        action: &ActionStmt,
        action_obj: &mut Map,
    ) -> Result<bool, RexRapError> {
        use runinator_models::functions::{
            FUNCTIONS_INVOKE, FUNCTIONS_NAMESPACE_PREFIX, FUNCTIONS_PROVIDER,
            FUNCTIONS_RUNNER_LABEL,
        };
        if !action.provider.starts_with(FUNCTIONS_NAMESPACE_PREFIX) {
            return Ok(false);
        }
        let entry = self
            .functions
            .iter()
            .filter(|entry| {
                entry.provider_name() == action.provider && entry.export_name == action.function
            })
            // an unversioned call takes the newest published version, and records it. resolving
            // once at compile time is the whole mechanism: nothing later re-resolves it.
            .max_by_key(|entry| entry.version)
            .ok_or_else(|| {
                RexRapError::Lower(format!(
                    "unknown packaged function '{}.{}'; publish it, or pass its catalog entry to the compiler",
                    action.provider, action.function
                ))
            })?;

        // the authored arguments become the handler's input rather than the action's parameters:
        // everything else in `configuration` is staging the worker owns, and an author's argument
        // named `handler` must not collide with it.
        let authored = action_obj
            .remove("configuration")
            .unwrap_or(Value::Object(Map::new()));
        let mut configuration = Map::new();
        configuration.insert("input".into(), authored);

        action_obj.insert("provider".into(), Value::String(FUNCTIONS_PROVIDER.into()));
        action_obj.insert("function".into(), Value::String(FUNCTIONS_INVOKE.into()));
        action_obj.insert("configuration".into(), Value::Object(configuration));
        action_obj.insert(
            "function_binding".into(),
            serde_json::to_value(entry.binding())
                .map(Value::from)
                .map_err(|err| {
                    RexRapError::Lower(format!("failed to encode function binding: {err}"))
                })?,
        );
        // running packaged code needs a container runtime, which not every worker has. an explicit
        // `.runner(...)` wins: an operator who pinned a pool meant it.
        if action.modifiers.runner.is_none() {
            let mut labels = Map::new();
            labels.insert(
                "runner".into(),
                Value::String(FUNCTIONS_RUNNER_LABEL.to_string()),
            );
            action_obj.insert("required_labels".into(), Value::Object(labels));
        }
        Ok(true)
    }

    /// lower the top-level workflow `params { }` type, attaching each field's default expression.
    /// defaults only exist on top-level parameter fields; nested struct fields go through plain
    /// type lowering.
    fn lower_input_type(
        &self,
        type_expr: &TypeExpr,
    ) -> Result<runinator_models::types::RuninatorType, RexRapError> {
        use runinator_models::types::{RuninatorField, RuninatorType};
        let TypeExpr::Struct { fields, additional } = type_expr else {
            return types::lower_type_with(type_expr, &self.named_types);
        };
        let mut mapped = std::collections::BTreeMap::new();
        for field in fields {
            let ty = types::lower_type_with(&field.ty, &self.named_types)?;
            let mut runinator_field = if field.optional {
                RuninatorField::optional(ty)
            } else {
                RuninatorField::required(ty)
            };
            if let Some(default) = &field.default {
                runinator_field = runinator_field.with_default(self.lower_expr(default)?);
            }
            mapped.insert(field.name.clone(), runinator_field);
        }
        let additional = additional
            .as_ref()
            .map(|ty| types::lower_type_with(ty, &self.named_types))
            .transpose()?
            .map(Box::new);
        Ok(RuninatorType::Struct {
            fields: mapped,
            additional,
        })
    }

    /// resolve `type <Name>` declarations into RuninatorType, rejecting cycles and duplicates.
    fn resolve_type_decls(&mut self, decls: &[TypeDecl]) -> Result<(), RexRapError> {
        self.named_types = types::resolve_named_types(decls)?;
        Ok(())
    }

    /// lower a declared type body using the resolved name table.
    fn lower_named_type(
        &self,
        type_expr: &TypeExpr,
    ) -> Result<runinator_models::types::RuninatorType, RexRapError> {
        types::lower_type_with(type_expr, &self.named_types)
    }

    /// lower header `trigger ...` declarations into runtime trigger specs. each spec carries a
    /// `kind` (`"cron"` or `"chained"`) so materialization can branch; cron/chained string operands
    /// must be string literals.
    fn lower_triggers(&self, triggers: &[TriggerDecl]) -> Result<Vec<Value>, RexRapError> {
        let mut specs = Vec::with_capacity(triggers.len());
        for trigger in triggers {
            let parameters = match &trigger.params {
                Some(params) => self.lower_expr(params)?,
                None => Value::Object(Map::new()),
            };
            let mut spec = Map::new();
            spec.insert("parameters".into(), parameters);
            spec.insert("enabled".into(), Value::Bool(trigger.enabled));
            match &trigger.kind {
                TriggerDeclKind::Cron {
                    schedule,
                    blackout_start,
                    blackout_end,
                    catchup,
                } => {
                    let Value::String(cron) = self.lower_expr(schedule)? else {
                        return Err(RexRapError::lower(
                            "trigger cron expression must be a string literal",
                        ));
                    };
                    spec.insert("kind".into(), Value::String("cron".into()));
                    spec.insert("cron".into(), Value::String(cron));
                    if let Some(start) = blackout_start {
                        let Value::String(start) = self.lower_expr(start)? else {
                            return Err(RexRapError::lower(
                                "trigger blackout start must be a string literal",
                            ));
                        };
                        spec.insert("blackout_start".into(), Value::String(start));
                    }
                    if let Some(end) = blackout_end {
                        let Value::String(end) = self.lower_expr(end)? else {
                            return Err(RexRapError::lower(
                                "trigger blackout end must be a string literal",
                            ));
                        };
                        spec.insert("blackout_end".into(), Value::String(end));
                    }
                    // the catch-up policy rides in the trigger's own configuration, so it lands in
                    // the same materialized row the cron expression does.
                    if let Some(catchup) = catchup {
                        let mut entry = Map::new();
                        entry.insert(
                            "policy".into(),
                            Value::String(catchup.policy.keyword().into()),
                        );
                        if let Some(grace) = catchup.grace_seconds {
                            entry.insert("grace_seconds".into(), Value::from(grace));
                        }
                        if let Some(max_slots) = catchup.max_slots {
                            entry.insert("max_slots".into(), Value::from(max_slots));
                        }
                        spec.insert("catchup".into(), Value::Object(entry));
                    }
                }
                TriggerDeclKind::Chained { event, target } => {
                    let Value::String(target) = self.lower_expr(target)? else {
                        return Err(RexRapError::lower(
                            "chained trigger target must be a string literal",
                        ));
                    };
                    spec.insert("kind".into(), Value::String("chained".into()));
                    spec.insert("on".into(), Value::String(event.as_str().into()));
                    spec.insert("target_workflow".into(), Value::String(target));
                }
            }
            specs.push(Value::Object(spec));
        }
        Ok(specs)
    }

    /// lower header `notify on <event> -> <channel> <target>` policies into `metadata.notifications`:
    /// `[{ event, channel, target, severity, threshold_seconds?, configuration?, enabled }]`. the
    /// web service upserts these as `managed_by = "rexrap"` rows on import.
    fn lower_notifications(&self, policies: &[NotifyDecl]) -> Result<Vec<Value>, RexRapError> {
        let mut specs = Vec::with_capacity(policies.len());
        for policy in policies {
            let Value::String(target) = self.lower_expr(&policy.target)? else {
                return Err(RexRapError::lower("notify target must be a string literal"));
            };
            let mut spec = Map::new();
            spec.insert(
                "event".into(),
                Value::String(policy.event.runtime_name().into()),
            );
            spec.insert(
                "channel".into(),
                Value::String(policy.channel.runtime_name().into()),
            );
            spec.insert("target".into(), Value::String(target));
            spec.insert(
                "severity".into(),
                Value::String(
                    policy
                        .severity
                        .clone()
                        .unwrap_or_else(|| "warning".to_string()),
                ),
            );
            if let Some(after) = policy.after_seconds {
                spec.insert("threshold_seconds".into(), Value::from(after));
            }
            if let Some(configuration) = &policy.configuration {
                spec.insert("configuration".into(), self.lower_expr(configuration)?);
            }
            spec.insert("enabled".into(), Value::Bool(policy.enabled));
            specs.push(Value::Object(spec));
        }
        Ok(specs)
    }

    /// lower header `watch <cond> -> <target>` guards into `metadata.watches`:
    /// `[{ condition: <lowered cond>, handler: <node id> }]`. the reducer re-evaluates each on every
    /// drive and jumps to the handler when the condition holds.
    fn lower_watches(&self, watches: &[WatchDecl]) -> Result<Vec<Value>, RexRapError> {
        let mut specs = Vec::with_capacity(watches.len());
        for watch in watches {
            let mut spec = Map::new();
            spec.insert("condition".into(), self.lower_cond(&watch.cond)?);
            spec.insert(
                "handler".into(),
                Value::String(self.target_id(&watch.handler)),
            );
            specs.push(Value::Object(spec));
        }
        Ok(specs)
    }

    /// lower user `fn` definitions into the `metadata.functions` runtime form:
    /// `[{ name, params: [{name}], body|program, recursive?: { max_depth } }]`. an expression body
    /// lowers to `body`; a block body lowers to a `program` array (the same `$let`/`$return`/`$if`
    /// form a `do` block produces). each body lowers with its parameters registered as locals,
    /// so param references become `let` refs the engine binds at call time.
    fn lower_functions(&self, functions: &[FunctionDef]) -> Result<Vec<Value>, RexRapError> {
        let mut out = Vec::with_capacity(functions.len());
        for def in functions {
            // snapshot locals so per-function params and block locals never leak across functions.
            let saved = self.compute_locals.borrow().clone();
            for param in &def.params {
                self.compute_locals.borrow_mut().insert(param.name.clone());
            }
            let (body_key, body_value) = match &def.body {
                FnBody::Expr(expr) => ("body", self.lower_expr(expr)),
                FnBody::Block(lines) => ("program", self.lower_fn_block(lines).map(Value::Array)),
                // a `task fn` is a graph region inlined at each call site; it is never a compute
                // intrinsic, so it never appears in `metadata.functions`.
                FnBody::Run(_) => continue,
            };
            self.compute_locals.replace(saved);
            let body_value = body_value?;
            let params = def
                .params
                .iter()
                .map(|param| {
                    Value::Object(Map::from_iter([(
                        "name".into(),
                        Value::String(param.name.clone()),
                    )]))
                })
                .collect();
            let mut entry = Map::new();
            entry.insert("name".into(), Value::String(def.name.clone()));
            entry.insert("params".into(), Value::Array(params));
            entry.insert(body_key.into(), body_value);
            if let Some(max_depth) = def.recursive {
                entry.insert(
                    "recursive".into(),
                    Value::Object(Map::from_iter([(
                        "max_depth".into(),
                        Value::from(max_depth as i64),
                    )])),
                );
            }
            out.push(Value::Object(entry));
        }
        Ok(out)
    }

    /// record a `let <id>: <type>` annotation for the graph metadata sidecar.
    fn record_declared_type(&mut self, id: &str, stmt: &Stmt) -> Result<(), RexRapError> {
        let Some(type_expr) = &stmt.label_type else {
            return Ok(());
        };
        // validate the annotation resolves, but record its name-preserving surface form so a
        // declared `type` reference (e.g. `let x: Cart`) decompiles back to the name, not its shape.
        let ty = types::lower_type_with(type_expr, &self.named_types)?;
        let rendered = runinator_rexrap_syntax::format::format_type(type_expr);
        self.declared_types
            .push((id.to_string(), Value::String(rendered)));
        let hint = Value::encode(&ty)
            .map_err(|err| RexRapError::lower(format!("failed to encode type hint: {err}")))?;
        self.declared_type_hints.push((id.to_string(), hint));
        Ok(())
    }

    fn record_generated_type_hint(
        &mut self,
        id: &str,
        action: &ActionStmt,
    ) -> Result<(), RexRapError> {
        let Some(metadata) = self
            .provider_actions
            .get(&(action.provider.clone(), action.function.clone()))
        else {
            return Ok(());
        };
        if metadata.results.is_empty() {
            return Ok(());
        }
        let ty = metadata.results_type();
        let hint = Value::encode(&ty)
            .map_err(|err| RexRapError::lower(format!("failed to encode type hint: {err}")))?;
        self.declared_type_hints.push((id.to_string(), hint));
        Ok(())
    }

    /// lower a sequence of statements, wiring each forward edge to the next statement's
    /// entry (or `cont` after the last). returns the block's entry node id.
    fn lower_block(&mut self, block: &[Stmt], cont: &str) -> Result<String, RexRapError> {
        if block.is_empty() {
            return Ok(cont.to_string());
        }
        // `async` launches fan out: a run of them (plus whatever sits between the launches and the
        // first statement that consumes one) becomes a `parallel` whose join is that consuming
        // statement. this is what makes two `async` calls actually overlap rather than merely
        // being spelled differently from two plain ones.
        let grouped = inline::group_async_launches(block);
        let block: &[Stmt] = &grouped;
        // pass 1: claim entry ids so forward references resolve.
        let mut entries = Vec::with_capacity(block.len());
        for stmt in block {
            entries.push(self.entry_id_for(stmt)?);
        }
        // pass 2: lower each statement with its concrete continuation.
        for (index, stmt) in block.iter().enumerate() {
            let next = if index + 1 < block.len() {
                entries[index + 1].entry.clone()
            } else {
                cont.to_string()
            };
            if let Some(collector) = &entries[index].collector {
                self.lower_stmt(stmt, &entries[index].entry, collector)?;
                self.lower_value_collector(stmt, collector, &next)?;
            } else {
                self.lower_stmt(stmt, &entries[index].entry, &next)?;
            }
        }
        Ok(entries[0].entry.clone())
    }

    fn entry_id_for(&mut self, stmt: &Stmt) -> Result<LowerEntry, RexRapError> {
        if let Some(id) = &stmt.annotations.id {
            let id = self.claim(id)?;
            if is_bound_control_stmt(stmt) {
                let collector = stmt
                    .label
                    .as_ref()
                    .map(|label| self.claim(label))
                    .transpose()?;
                return Ok(LowerEntry {
                    entry: id,
                    collector,
                });
            }
            if is_control_stmt(stmt) {
                self.control_ids.push(id.clone());
            }
            return Ok(LowerEntry {
                entry: id,
                collector: None,
            });
        }
        if let Some(label) = &stmt.label {
            if is_control_stmt(stmt) {
                let collector = self.claim(label)?;
                return Ok(LowerEntry {
                    entry: self.fresh(control_prefix(&stmt.kind)),
                    collector: Some(collector),
                });
            }
            return Ok(LowerEntry {
                entry: self.claim(label)?,
                collector: None,
            });
        }
        // `resume` takes its id from a counter of its own rather than the shared one, because the
        // shared counter is not reproducible across a round trip: decompile writes most node ids out
        // explicitly (`node action_1 <- ...`), and claiming an explicit id does not advance the
        // counter. a `resume` sitting after such a node would therefore come back numbered
        // differently, diverging the round trip on nothing but its id. counting resumes alone is
        // stable, because decompile preserves how many there are and what order they appear in.
        if matches!(stmt.kind, StmtKind::Resume(_)) {
            return Ok(LowerEntry {
                entry: self.fresh_resume(),
                collector: None,
            });
        }
        Ok(LowerEntry {
            entry: self.fresh(control_prefix(&stmt.kind)),
            collector: None,
        })
    }

    fn lower_stmt(&mut self, stmt: &Stmt, id: &str, next: &str) -> Result<(), RexRapError> {
        // a nested block re-enters here, so save and restore rather than assign: an inner statement
        // must not leave its span attached to the rest of its parent's nodes.
        let outer_span = self.current_span.replace(stmt.span);
        let result = self.lower_stmt_inner(stmt, id, next);
        self.current_span = outer_span;
        result
    }

    fn lower_stmt_inner(&mut self, stmt: &Stmt, id: &str, next: &str) -> Result<(), RexRapError> {
        // an `async` call binds a task handle, whatever the callee is; `await`/`detach` resolve
        // against this. the marker is on the call site, so the callee never has to declare a color.
        if stmt.is_async {
            if let Some(label) = &stmt.label {
                let binding = match &stmt.kind {
                    StmtKind::Subflow(_) => TaskBinding::Subflow,
                    _ => TaskBinding::Provider,
                };
                self.task_bindings.insert(label.clone(), binding);
            }
        }
        match &stmt.kind {
            StmtKind::Action(action) => self.lower_action(action, stmt, id, next),
            StmtKind::TaskCall(call) => self.lower_task_call(call, stmt, id, next),
            StmtKind::Compute(compute) => self.lower_compute(compute, stmt, id, next),
            StmtKind::Subflow(subflow) => self.lower_subflow(subflow, stmt, id, next),
            StmtKind::Wait(wait) => self.lower_wait(wait, stmt, id, next),
            StmtKind::Output(output) => self.lower_output(output, stmt, id, next),
            StmtKind::Yield(value) => self.lower_yield(value, stmt, id, next),
            StmtKind::Input(input) => self.lower_input(input, stmt, id, next),
            StmtKind::Approval(approval) => self.lower_approval(approval, stmt, id, next),
            StmtKind::Gate(gate) => self.lower_gate(gate, stmt, id, next),
            StmtKind::Signal(signal) => self.lower_signal(signal, stmt, id, next),
            StmtKind::Assert(assert) => self.lower_assert(assert, stmt, id, next),
            StmtKind::Transform(transform) => self.lower_transform(transform, stmt, id, next),
            StmtKind::Audit(audit) => self.lower_audit(audit, stmt, id, next),
            StmtKind::Checkpoint(checkpoint) => self.lower_checkpoint(checkpoint, stmt, id, next),
            StmtKind::Mutex(mutex) => self.lower_mutex(mutex, stmt, id, next),
            StmtKind::Throttle(throttle) => self.lower_throttle(throttle, stmt, id, next),
            StmtKind::Cooldown(cooldown) => self.lower_cooldown(cooldown, stmt, id, next),
            StmtKind::Await(await_stmt) => self.lower_await(await_stmt, stmt, id, next),
            StmtKind::Debounce(debounce) => self.lower_debounce(debounce, stmt, id, next),
            StmtKind::Collect(collect) => self.lower_collect(collect, stmt, id, next),
            StmtKind::Barrier(barrier) => self.lower_barrier(barrier, stmt, id, next),
            StmtKind::CircuitBreaker(cb) => self.lower_circuit_breaker(cb, stmt, id, next),
            StmtKind::EventSource(es) => self.lower_event_source(es, stmt, id, next),
            StmtKind::Config(config) => self.lower_config(config, stmt, id, next),
            StmtKind::Return(value) => self.lower_return(value.as_ref(), stmt, id),
            StmtKind::Detach(handle) => self.lower_detach(handle, stmt, id, next),
            StmtKind::Fail(message) => self.lower_fail(message.as_ref(), stmt, id),
            StmtKind::Resume(resume) => self.lower_resume(resume, stmt, id),
            StmtKind::If(if_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_if(if_stmt, stmt, id, &cont)
            }
            StmtKind::For(for_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_for(for_stmt, stmt, id, &cont)
            }
            StmtKind::While(while_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_while(while_stmt, stmt, id, &cont)
            }
            StmtKind::Match(match_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_match(match_stmt, stmt, id, &cont)
            }
            StmtKind::Parallel(parallel) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_parallel(parallel, stmt, id, &cont)
            }
            StmtKind::Try(try_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_try(try_stmt, stmt, id, &cont)
            }
            StmtKind::Race(race) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_race(race, stmt, id, &cont)
            }
            StmtKind::Map(map_stmt) => {
                let cont = self.block_cont(&stmt.transitions, next);
                self.lower_map(map_stmt, stmt, id, &cont)
            }
        }
    }

    /// `return <expr>?` — supply the run's result and continue to the generated `end` terminal.
    /// it is a compute node like `yield`, but its continuation is always the successful terminal
    /// rather than the next sibling, which is what makes it a concise terminal form.
    fn lower_return(
        &mut self,
        value: Option<&Expr>,
        stmt: &Stmt,
        id: &str,
    ) -> Result<(), RexRapError> {
        let end = self.end_id.clone();
        let body = match value {
            Some(value) => vec![ComputeLine::Return(value.clone())],
            None => Vec::new(),
        };
        let compute = ComputeStmt {
            body,
            foreign: None,
            modifiers: Modifiers::default(),
        };
        self.lower_compute(&compute, stmt, id, &end)
    }

    /// `detach <handle>` — drop an `async` handle without joining it. the launch still runs; the
    /// statement itself is a pure no-op node that records the intent so decompile can restore it.
    fn lower_detach(
        &mut self,
        handle: &str,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        if !self.task_bindings.contains_key(handle) {
            return Err(RexRapError::semantic(
                stmt.span,
                format!("`detach {handle}` must reference an earlier `async` binding"),
            ));
        }
        self.detached.insert(handle.to_string());
        let mut params = Map::new();
        params.insert("detach".into(), Value::String(handle.to_string()));
        params.insert("bindings".into(), Value::Object(Map::new()));
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "transform", fields));
        Ok(())
    }

    fn lower_yield(
        &mut self,
        value: &Expr,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let compute = ComputeStmt {
            body: vec![ComputeLine::Return(value.clone())],
            foreign: None,
            modifiers: Modifiers::default(),
        };
        self.lower_compute(&compute, stmt, id, next)
    }

    fn lower_value_collector(
        &mut self,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let span = stmt.span;
        let value = control_value_expr(&stmt.kind);
        let synthetic = Stmt {
            is_async: false,
            span,
            annotations: Annotations::default(),
            label: stmt.label.clone(),
            label_type: stmt.label_type.clone(),
            kind: StmtKind::Compute(ComputeStmt {
                body: vec![ComputeLine::Return(value)],
                foreign: None,
                modifiers: Modifiers::default(),
            }),
            transitions: TransitionClause::default(),
            compensation: None,
            comments: runinator_rexrap_syntax::comments::CommentSet::default(),
        };
        // the synthetic statement is built as a compute above; guard the invariant instead of
        // panicking if that ever changes.
        let StmtKind::Compute(compute) = &synthetic.kind else {
            return Err(RexRapError::lower(
                "synthetic value collector statement must be a compute statement",
            ));
        };
        self.lower_compute(compute, &synthetic, id, next)
    }

    // leaf statements -------------------------------------------------------

    /// lower a `task fn` call by splicing the callee's body into the caller's graph.
    ///
    /// the arguments are substituted into the body rather than passed at runtime, because an
    /// inlined region has no frame to pass them in. the region's own labels are namespaced by the
    /// call site's node id, so calling the same `task fn` twice cannot collide.
    fn lower_task_call(
        &mut self,
        call: &TaskCallStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let Some(def) = self.task_fns.get(&call.name).cloned() else {
            return Err(RexRapError::semantic(
                stmt.span,
                format!("unknown `task fn` '{}'", call.name),
            ));
        };
        let FnBody::Run(body) = &def.body else {
            return Err(RexRapError::semantic(
                stmt.span,
                format!("'{}' is not a `task fn`", call.name),
            ));
        };
        // bind by name, falling back to each parameter's declared default.
        let flat = runinator_rexrap_sema::desugar::flatten_entries(&call.args, &self.aliases)?;
        let supplied: HashMap<String, Expr> = flat.into_iter().collect();
        let mut bindings = HashMap::new();
        for param in &def.params {
            let value = supplied
                .get(&param.name)
                .cloned()
                .or_else(|| param.default.clone());
            match value {
                Some(value) => {
                    bindings.insert(param.name.clone(), value);
                }
                None if param.optional => {}
                None => {
                    return Err(RexRapError::semantic(
                        stmt.span,
                        format!("`{}` is missing argument '{}'", call.name, param.name),
                    ));
                }
            }
        }
        for name in supplied.keys() {
            if !def.params.iter().any(|param| &param.name == name) {
                return Err(RexRapError::semantic(
                    stmt.span,
                    format!("`{}` has no parameter '{name}'", call.name),
                ));
            }
        }
        let inlined = inline::inline_body(body, &bindings, id)?;
        // the call site's own id fronts the region so transitions targeting the call still land.
        let entry = self.lower_block(&inlined, next)?;
        let mut params = Map::new();
        params.insert("bindings".into(), Value::Object(Map::new()));
        self.push(node(
            id,
            "transform",
            vec![
                ("parameters", Value::Object(params)),
                ("transitions", transitions_next(&entry)),
            ],
        ));
        Ok(())
    }

    fn lower_action(
        &mut self,
        action: &ActionStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        self.record_declared_type(id, stmt)?;
        if matches!(stmt.label_type, Some(TypeExpr::Task(_))) {
            let label = stmt
                .label
                .as_ref()
                .ok_or_else(|| RexRapError::semantic(stmt.span, "a task binding needs a name"))?;
            self.task_bindings
                .insert(label.clone(), TaskBinding::Provider);
        }
        if stmt.label_type.is_none() {
            self.record_generated_type_hint(id, action)?;
        }
        // expand `...alias` spreads for the graph, and record the authored form for resugaring.
        let flat = runinator_rexrap_sema::desugar::flatten_entries(&action.args, &self.aliases)?;
        let mut config = Map::new();
        for (name, value) in &flat {
            config.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &action.args)?;
        let mut action_obj = Map::new();
        action_obj.insert("provider".into(), Value::String(action.provider.clone()));
        action_obj.insert("function".into(), Value::String(action.function.clone()));
        action_obj.insert(
            "timeout_seconds".into(),
            Value::from(action.modifiers.timeout_seconds.unwrap_or(60)),
        );
        action_obj.insert("configuration".into(), Value::Object(config));
        if action.modifiers.mcp {
            action_obj.insert("mcp_enabled".into(), Value::Bool(true));
        }
        if !action.modifiers.tags.is_empty() {
            action_obj.insert(
                "tags".into(),
                Value::Array(
                    action
                        .modifiers
                        .tags
                        .iter()
                        .map(|tag| Value::String(tag.clone()))
                        .collect(),
                ),
            );
        }
        if let Some(runner) = &action.modifiers.runner {
            let mut labels = Map::new();
            labels.insert("runner".into(), Value::String(runner.clone()));
            action_obj.insert("required_labels".into(), Value::Object(labels));
        }
        if let Some(key) = &action.modifiers.idempotency_key {
            let lowered = self.lower_expr(key)?;
            action_obj.insert("idempotency_key".into(), lowered);
        }
        self.apply_function_binding(action, &mut action_obj)?;

        let mut fields = vec![
            ("action", Value::Object(action_obj)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        // This marker is intentionally node metadata rather than an action property: workers run
        // the exact same provider action; only orchestration differs (the cursor advances once the
        // durable task record has been dispatched).
        if matches!(stmt.label_type, Some(TypeExpr::Task(_))) {
            fields.push((
                "parameters",
                Value::Object(Map::from_iter([("rexrap_task".into(), Value::Bool(true))])),
            ));
        }
        if let Some(compensation) = &stmt.compensation {
            fields.push(("compensation", self.lower_action_object(compensation)?));
        }
        self.apply_modifier_fields(&mut fields, &action.modifiers);
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "action", fields));
        Ok(())
    }

    /// lower a bare action call (provider/function/args/modifiers) into a `WorkflowAction`-shaped
    /// object. used for `compensate` actions, which carry no node identity, transitions, or spreads.
    fn lower_action_object(&mut self, action: &ActionStmt) -> Result<Value, RexRapError> {
        let flat = runinator_rexrap_sema::desugar::flatten_entries(&action.args, &self.aliases)?;
        let mut config = Map::new();
        for (name, value) in &flat {
            config.insert(name.clone(), self.lower_expr(value)?);
        }
        let mut obj = Map::new();
        obj.insert("provider".into(), Value::String(action.provider.clone()));
        obj.insert("function".into(), Value::String(action.function.clone()));
        obj.insert(
            "timeout_seconds".into(),
            Value::from(action.modifiers.timeout_seconds.unwrap_or(60)),
        );
        obj.insert("configuration".into(), Value::Object(config));
        if action.modifiers.mcp {
            obj.insert("mcp_enabled".into(), Value::Bool(true));
        }
        if !action.modifiers.tags.is_empty() {
            obj.insert(
                "tags".into(),
                Value::Array(
                    action
                        .modifiers
                        .tags
                        .iter()
                        .map(|tag| Value::String(tag.clone()))
                        .collect(),
                ),
            );
        }
        if let Some(runner) = &action.modifiers.runner {
            let mut labels = Map::new();
            labels.insert("runner".into(), Value::String(runner.clone()));
            obj.insert("required_labels".into(), Value::Object(labels));
        }
        if let Some(key) = &action.modifiers.idempotency_key {
            let lowered = self.lower_expr(key)?;
            obj.insert("idempotency_key".into(), lowered);
        }
        // the compensate path goes through the same helper: a compensating packaged-function call
        // is as much a packaged-function call as the forward one, and this is easy to miss.
        self.apply_function_binding(action, &mut obj)?;
        Ok(Value::Object(obj))
    }

    fn lower_subflow(
        &mut self,
        subflow: &SubflowStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        self.record_declared_type(id, stmt)?;
        if matches!(stmt.label_type, Some(TypeExpr::Task(_))) {
            if !subflow.detached {
                return Err(RexRapError::semantic(
                    stmt.span,
                    "a task binding must start a detached subflow",
                ));
            }
            let label = stmt
                .label
                .as_ref()
                .ok_or_else(|| RexRapError::semantic(stmt.span, "a task binding needs a name"))?;
            self.task_bindings
                .insert(label.clone(), TaskBinding::Subflow);
        }
        let mut subflow_obj = Map::new();
        subflow_obj.insert(
            "workflow_name".into(),
            Value::String(subflow.workflow_name.clone()),
        );
        subflow_obj.insert(
            "type".into(),
            Value::String(if subflow.detached {
                "fire_and_forget".into()
            } else {
                "wait".into()
            }),
        );
        if subflow.reuse {
            subflow_obj.insert("reuse_open_run".into(), Value::Bool(true));
        }
        if let Some(run_name) = &subflow.run_name {
            subflow_obj.insert("run_name".into(), self.lower_expr(run_name)?);
        }

        let flat = runinator_rexrap_sema::desugar::flatten_entries(&subflow.params, &self.aliases)?;
        let mut params = Map::new();
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &subflow.params)?;

        let mut fields = vec![
            ("subflow", Value::Object(subflow_obj)),
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "subflow", fields));
        Ok(())
    }

    fn lower_wait(
        &mut self,
        wait: &WaitStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut wait_obj = Map::new();
        let seconds = match &wait.amount {
            WaitAmount::Seconds(seconds) => Value::from(*seconds),
            WaitAmount::Expr(expr) => self.lower_expr(expr)?,
        };
        wait_obj.insert("seconds".into(), seconds);
        if let Some(status) = &wait.until_status {
            wait_obj.insert("until_status".into(), Value::String(status.clone()));
        }
        if let Some(status) = &wait.initial_status {
            wait_obj.insert("initial_status".into(), Value::String(status.clone()));
        }
        let mut fields = vec![
            ("wait", Value::Object(wait_obj)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "wait", fields));
        Ok(())
    }

    fn lower_output(
        &mut self,
        output: &OutputStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        if let Some(event_type) = &output.event_type {
            params.insert("event_type".into(), Value::String(event_type.clone()));
        }
        let data = match &output.data {
            Some(data) => self.lower_expr(data)?,
            None => Value::Null,
        };
        params.insert("data".into(), data);
        if !output.items.is_empty() {
            let mut items = Vec::with_capacity(output.items.len());
            for (name, source) in &output.items {
                let mut entry = Map::new();
                entry.insert("name".into(), Value::String(name.clone()));
                entry.insert("source".into(), self.lower_expr(source)?);
                items.push(Value::Object(entry));
            }
            params.insert("items".into(), Value::Array(items));
        }
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "output", fields));
        Ok(())
    }

    fn lower_input(
        &mut self,
        input: &InputStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        if let Some(prompt) = &input.prompt {
            params.insert("prompt".into(), self.lower_expr(prompt)?);
        }
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "input", fields));
        Ok(())
    }

    fn lower_approval(
        &mut self,
        approval: &ApprovalStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert(
            "approval_type".into(),
            Value::String(
                approval
                    .approval_type
                    .clone()
                    .unwrap_or_else(|| "generic".into()),
            ),
        );
        params.insert("prompt".into(), self.lower_expr(&approval.prompt)?);
        let flat =
            runinator_rexrap_sema::desugar::flatten_entries(&approval.metadata, &self.aliases)?;
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &approval.metadata)?;
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "approval", fields));
        Ok(())
    }

    fn lower_gate(
        &mut self,
        gate: &GateStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("kind".into(), Value::String(gate.kind.clone()));
        if let Some(when) = &gate.when {
            params.insert("when".into(), self.lower_cond(when)?);
        }
        if let Some(poll) = gate.poll_interval {
            params.insert("poll_interval".into(), Value::from(poll));
        }
        if let Some(timeout) = gate.timeout {
            params.insert("timeout".into(), Value::from(timeout));
        }
        if let Some(timeout_policy) = &gate.timeout_policy {
            params.insert(
                "timeout_policy".into(),
                Value::String(timeout_policy.clone()),
            );
        }
        let flat = runinator_rexrap_sema::desugar::flatten_entries(&gate.metadata, &self.aliases)?;
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &gate.metadata)?;
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "gate", fields));
        Ok(())
    }

    fn lower_signal(
        &mut self,
        signal: &SignalStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(signal.name.clone()));
        if let Some(key) = &signal.correlation_key {
            // lowered as an expression (often a ref like `params.ticket.key`); the reducer resolves
            // it against the runtime context when the node parks.
            params.insert("correlation_key".into(), self.lower_expr(key)?);
        }
        let flat =
            runinator_rexrap_sema::desugar::flatten_entries(&signal.metadata, &self.aliases)?;
        for (name, value) in &flat {
            params.insert(name.clone(), self.lower_expr(value)?);
        }
        self.record_spreads(id, &signal.metadata)?;
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "signal", fields));
        Ok(())
    }

    // build the standard fields for a leaf coordination/resilience node: parameters, on_success
    // transitions, an optional node timeout, and annotations. keeps the lower_* bodies focused on
    // their parameter shapes.
    fn leaf_fields(
        &self,
        params: Map,
        stmt: &Stmt,
        next: &str,
        timeout: Option<i64>,
    ) -> Result<Vec<(&'static str, Value)>, RexRapError> {
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "on_success", next)?,
            ),
        ];
        if let Some(seconds) = timeout {
            fields.push(("timeout_seconds", Value::from(seconds)));
        }
        self.apply_annotations(&mut fields, stmt);
        Ok(fields)
    }

    fn lower_assert(
        &mut self,
        assert: &AssertStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut assertions = Vec::with_capacity(assert.assertions.len());
        for (name, cond) in &assert.assertions {
            let mut entry = Map::new();
            entry.insert("name".into(), Value::String(name.clone()));
            entry.insert("condition".into(), self.lower_cond(cond)?);
            entry.insert("message".into(), Value::String(name.clone()));
            assertions.push(Value::Object(entry));
        }
        let mut params = Map::new();
        params.insert("assertions".into(), Value::Array(assertions));
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "assert", fields));
        Ok(())
    }

    fn lower_transform(
        &mut self,
        transform: &TransformStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut bindings = Map::new();
        for (name, value) in &transform.bindings {
            bindings.insert(name.clone(), self.lower_expr(value)?);
        }
        let mut params = Map::new();
        params.insert("bindings".into(), Value::Object(bindings));
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "transform", fields));
        Ok(())
    }

    fn lower_audit(
        &mut self,
        audit: &AuditStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("action".into(), self.lower_expr(&audit.action)?);
        if let Some(actor) = &audit.actor {
            params.insert("actor".into(), self.lower_expr(actor)?);
        }
        if let Some(target) = &audit.target {
            params.insert("target".into(), self.lower_expr(target)?);
        }
        if let Some(reason) = &audit.reason {
            params.insert("reason".into(), self.lower_expr(reason)?);
        }
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "audit", fields));
        Ok(())
    }

    fn lower_checkpoint(
        &mut self,
        checkpoint: &CheckpointStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(checkpoint.name.clone()));
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "checkpoint", fields));
        Ok(())
    }

    fn lower_mutex(
        &mut self,
        mutex: &MutexStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        // a bare release leaf ends a section; it carries only the lock name.
        if mutex.release {
            let mut params = Map::new();
            params.insert("name".into(), Value::String(mutex.name.clone()));
            params.insert("release".into(), Value::Bool(true));
            let fields = self.leaf_fields(params, stmt, next, None)?;
            self.push(node(id, "mutex", fields));
            return Ok(());
        }

        // acquire parameters shared by the leaf and block forms.
        let mut params = Map::new();
        params.insert("name".into(), Value::String(mutex.name.clone()));
        if let Some(poll) = mutex.poll_interval {
            params.insert("poll_interval_seconds".into(), Value::from(poll));
        }
        if let Some(hold) = mutex.hold {
            params.insert("hold_timeout_seconds".into(), Value::from(hold));
        }

        // acquire-only leaf: hold the lock until the run terminates.
        if mutex.body.is_empty() {
            let fields = self.leaf_fields(params, stmt, next, mutex.timeout)?;
            self.push(node(id, "mutex", fields));
            return Ok(());
        }

        // block form: acquire -> body -> release -> continuation. mirror the parallel/join id scheme so
        // the synthetic release id stays stable across a round trip.
        let cont = self.block_cont(&stmt.transitions, next);
        let release_id = self
            .claim(&format!("{id}_release"))
            .unwrap_or_else(|_| self.fresh("mutex_release"));
        let body_entry = self.lower_block(&mutex.body, &release_id)?;

        let mut acquire_transitions = Map::new();
        acquire_transitions.insert("next".into(), node_ref(&body_entry));
        let mut acquire_fields = vec![
            ("parameters", Value::Object(params)),
            ("transitions", Value::Object(acquire_transitions)),
        ];
        if let Some(seconds) = mutex.timeout {
            acquire_fields.push(("timeout_seconds", Value::from(seconds)));
        }
        self.apply_annotations(&mut acquire_fields, stmt);
        self.push(node(id, "mutex", acquire_fields));

        let mut release_params = Map::new();
        release_params.insert("name".into(), Value::String(mutex.name.clone()));
        release_params.insert("release".into(), Value::Bool(true));
        let mut release_transitions = Map::new();
        release_transitions.insert("next".into(), node_ref(&cont));
        self.push(node(
            &release_id,
            "mutex",
            vec![
                ("parameters", Value::Object(release_params)),
                ("transitions", Value::Object(release_transitions)),
            ],
        ));
        Ok(())
    }

    fn lower_throttle(
        &mut self,
        throttle: &ThrottleStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(throttle.name.clone()));
        params.insert(
            "max_per_window".into(),
            Value::from(throttle.max_per_window),
        );
        params.insert(
            "window_seconds".into(),
            Value::from(throttle.window_seconds),
        );
        if let Some(poll) = throttle.poll_interval {
            params.insert("poll_interval_seconds".into(), Value::from(poll));
        }
        let fields = self.leaf_fields(params, stmt, next, throttle.timeout)?;
        self.push(node(id, "throttle", fields));
        Ok(())
    }

    fn lower_cooldown(
        &mut self,
        cooldown: &CooldownStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(cooldown.name.clone()));
        params.insert(
            "window_seconds".into(),
            Value::from(cooldown.window_seconds),
        );
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "cooldown", fields));
        Ok(())
    }

    fn lower_await(
        &mut self,
        await_stmt: &AwaitStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        match &await_stmt.target {
            AwaitTarget::Workflow(workflow) => {
                params.insert("workflow".into(), Value::String(workflow.clone()));
            }
            AwaitTarget::Task(task) => {
                let Some(binding) = self.task_bindings.get(task) else {
                    return Err(RexRapError::semantic(
                        stmt.span,
                        format!("`await {task}` must reference an earlier task binding"),
                    ));
                };
                let field = match binding {
                    TaskBinding::Subflow => "subflow_run_id",
                    TaskBinding::Provider => "task_run_id",
                };
                let task_run_id = Expr::new(
                    ExprKind::Path(vec![PathSeg::Key(task.clone()), PathSeg::Key(field.into())]),
                    stmt.span,
                );
                params.insert(
                    match binding {
                        TaskBinding::Subflow => "run_id",
                        TaskBinding::Provider => "task_run_id",
                    }
                    .into(),
                    self.lower_expr(&task_run_id)?,
                );
            }
        }
        if let Some(key) = &await_stmt.key {
            params.insert("key".into(), self.lower_expr(key)?);
        }
        if let Some(mode) = &await_stmt.mode {
            params.insert("mode".into(), Value::String(mode.clone()));
        }
        let fields = self.leaf_fields(params, stmt, next, await_stmt.timeout)?;
        self.push(node(id, "await_run", fields));
        Ok(())
    }

    fn lower_debounce(
        &mut self,
        debounce: &DebounceStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(debounce.name.clone()));
        params.insert("delay_seconds".into(), Value::from(debounce.delay_seconds));
        if let Some(key) = &debounce.key {
            params.insert("trigger_key".into(), self.lower_expr(key)?);
        }
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "debounce", fields));
        Ok(())
    }

    fn lower_collect(
        &mut self,
        collect: &CollectStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(collect.name.clone()));
        params.insert("max".into(), Value::from(collect.max));
        let fields = self.leaf_fields(params, stmt, next, collect.timeout)?;
        self.push(node(id, "collect", fields));
        Ok(())
    }

    fn lower_barrier(
        &mut self,
        barrier: &BarrierStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(barrier.name.clone()));
        params.insert("count".into(), Value::from(barrier.count));
        if let Some(poll) = barrier.poll_interval {
            params.insert("poll_interval_seconds".into(), Value::from(poll));
        }
        let fields = self.leaf_fields(params, stmt, next, barrier.timeout)?;
        self.push(node(id, "barrier", fields));
        Ok(())
    }

    fn lower_circuit_breaker(
        &mut self,
        cb: &CircuitBreakerStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("name".into(), Value::String(cb.name.clone()));
        params.insert("threshold".into(), Value::from(cb.threshold));
        params.insert("window_seconds".into(), Value::from(cb.window_seconds));
        params.insert("cooldown_seconds".into(), Value::from(cb.cooldown_seconds));
        let fields = self.leaf_fields(params, stmt, next, None)?;
        self.push(node(id, "circuit_breaker", fields));
        Ok(())
    }

    fn lower_event_source(
        &mut self,
        es: &EventSourceStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        params.insert("event_type".into(), Value::String(es.event_type.clone()));
        if let Some(filter) = &es.filter {
            params.insert("filter".into(), self.lower_cond(filter)?);
        }
        if let Some(max) = es.max {
            params.insert("max".into(), Value::from(max));
        }
        let fields = self.leaf_fields(params, stmt, next, es.timeout)?;
        self.push(node(id, "event_source", fields));
        Ok(())
    }

    fn lower_config(
        &mut self,
        config: &ConfigStmt,
        stmt: &Stmt,
        id: &str,
        next: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        if let Some(name) = &config.name {
            params.insert("name".into(), self.lower_expr(name)?);
        }
        if let Some(metadata) = &config.metadata {
            params.insert("metadata".into(), self.lower_expr(metadata)?);
        }
        let mut fields = vec![
            ("parameters", Value::Object(params)),
            (
                "transitions",
                self.leaf_transitions(&stmt.transitions, "next", next)?,
            ),
        ];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "config", fields));
        Ok(())
    }

    fn lower_fail(
        &mut self,
        message: Option<&Expr>,
        stmt: &Stmt,
        id: &str,
    ) -> Result<(), RexRapError> {
        let mut fields = Vec::new();
        if let Some(message) = message {
            let mut params = Map::new();
            params.insert("message".into(), self.lower_expr(message)?);
            fields.push(("parameters", Value::Object(params)));
        }
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "fail", fields));
        Ok(())
    }

    /// `resume [mode]` lowers to a terminal `resume` node. like `fail` it takes no outgoing edge:
    /// it ends its thread of control by handing it back, so there is nothing downstream of it.
    fn lower_resume(
        &mut self,
        resume: &ResumeStmt,
        stmt: &Stmt,
        id: &str,
    ) -> Result<(), RexRapError> {
        let mut params = Map::new();
        // the bare form is the default; write it explicitly so the compiled node is self-describing.
        let mode = match resume.mode.as_deref() {
            Some("next") => "continue",
            Some(other) => other,
            None => "resume",
        };
        params.insert("mode".into(), Value::String(mode.to_string()));
        let mut fields = vec![("parameters", Value::Object(params))];
        self.apply_annotations(&mut fields, stmt);
        self.push(node(id, "resume", fields));
        Ok(())
    }

    /// lower header `interrupt on <source> { ... }` regions.
    ///
    /// each region gets an `interrupt` entry node whose `next` is the block's first statement. the
    /// graph describes the region, while metadata links a source to its entry and carries whether
    /// that link is enabled.
    ///
    /// the block's continuation is a synthetic terminal `resume`, which is what makes "every path
    /// out of a region hands control back" true by construction rather than by the author's care —
    /// a branch that just runs off the end of the block lands there instead of dangling.
    /// lower each `join <name> { … }` continuation into the same node list as the main flow.
    /// like an interrupt region it is unreachable from `start` — only an explicit `continue <name>`
    /// route enters it. the region is fronted by a pass-through node carrying the join's name, so
    /// the name is a stable transition target no matter how the first body statement is bound.
    fn lower_joins(&mut self, joins: &[JoinDecl], cont: &str) -> Result<Vec<Value>, RexRapError> {
        let mut names = Vec::with_capacity(joins.len());
        for join in joins {
            let body = self.lower_block(&join.body, cont)?;
            // claimed after the body so the region's own generated ids are unaffected by it.
            let entry = self.claim(&join.name)?;
            let mut params = Map::new();
            params.insert("bindings".into(), Value::Object(Map::new()));
            self.push(node(
                &entry,
                "transform",
                vec![
                    ("parameters", Value::Object(params)),
                    ("transitions", transitions_next(&body)),
                ],
            ));
            names.push(Value::String(join.name.clone()));
        }
        Ok(names)
    }

    fn lower_interrupts(
        &mut self,
        interrupts: &[InterruptDecl],
    ) -> Result<Vec<Value>, RexRapError> {
        let mut specs = Vec::with_capacity(interrupts.len());
        for (index, interrupt) in interrupts.iter().enumerate() {
            let implicit = self.claim(&format!("__interrupt_{}_resume", index))?;
            let body = self.lower_block(&interrupt.body, &implicit)?;
            // only materialize the synthetic terminal when something can actually reach it; a block
            // ending in an explicit `resume` would otherwise leave an orphan node behind.
            if body == implicit || !ends_in_resume(&interrupt.body) {
                let mut params = Map::new();
                params.insert("mode".into(), Value::String("resume".into()));
                self.push(node(
                    &implicit,
                    "resume",
                    vec![("parameters", Value::Object(params))],
                ));
            }
            // claimed after the body so the region's own ids are unaffected by its presence. the
            // id is derived from the index rather than a counter, so it is stable across a decompile
            // and recompile — the same reason the synthetic resume above is named this way.
            let entry = self.claim(&format!("__interrupt_{}_entry", index))?;
            let mut transitions = Map::new();
            transitions.insert("next".into(), node_ref(&body));
            self.push(node(
                &entry,
                "interrupt",
                vec![("transitions", Value::Object(transitions))],
            ));

            let mut spec = Map::new();
            spec.insert("on".into(), Value::String(interrupt.source.clone()));
            spec.insert("handler".into(), Value::String(entry));
            if !interrupt.enabled {
                spec.insert("enabled".into(), Value::Bool(false));
            }
            specs.push(Value::Object(spec));
        }
        Ok(specs)
    }

    // shared helpers --------------------------------------------------------

    fn apply_modifier_fields(
        &self,
        fields: &mut Vec<(&'static str, Value)>,
        modifiers: &Modifiers,
    ) {
        if let Some(retry) = &modifiers.retry {
            let mut obj = Map::new();
            obj.insert("max_attempts".into(), Value::from(retry.max_attempts));
            if let Some(base) = retry.backoff_base_seconds {
                obj.insert("backoff_base_seconds".into(), Value::from(base));
            }
            if let Some(max) = retry.backoff_max_seconds {
                obj.insert("backoff_max_seconds".into(), Value::from(max));
            }
            if retry.jitter {
                obj.insert("jitter".into(), Value::Bool(true));
            }
            if let Some(on) = &retry.retry_on {
                obj.insert("retry_on".into(), Value::from(on.clone()));
            }
            fields.push(("retry", Value::Object(obj)));
        }
        if let Some(reentry) = &modifiers.reentry {
            let mut obj = Map::new();
            obj.insert("enabled".into(), Value::Bool(true));
            obj.insert("max_visits".into(), Value::from(reentry.max_visits));
            if let Some(target) = &reentry.on_exhausted {
                obj.insert("on_exhausted".into(), node_ref(&self.target_id(target)));
            }
            fields.push(("reentry", Value::Object(obj)));
        }
    }

    pub(super) fn apply_annotations(&self, fields: &mut Vec<(&'static str, Value)>, stmt: &Stmt) {
        if stmt.annotations.skip {
            fields.push(("skipped", Value::Bool(true)));
        }
        if stmt.annotations.locked {
            fields.push(("locked", Value::Bool(true)));
        }
        if let Some(timeout) = stmt.annotations.timeout_seconds {
            fields.push(("timeout_seconds", Value::from(timeout)));
        }
    }

    /// build a transitions object for a leaf step. the happy path uses `primary`
    /// (on_success for actions, next for control-ish leaves) and falls back to `cont`.
    fn leaf_transitions(
        &self,
        clause: &TransitionClause,
        primary: &str,
        cont: &str,
    ) -> Result<Value, RexRapError> {
        let mut map = Map::new();
        let success = clause.next.as_ref().or(clause.on_success.as_ref());
        let success_id = match success {
            Some(target) => self.target_id(target),
            None => cont.to_string(),
        };
        map.insert(primary.to_string(), node_ref(&success_id));
        if let Some(target) = &clause.on_failure {
            map.insert("on_failure".into(), node_ref(&self.target_id(target)));
        }
        if let Some(target) = &clause.on_timeout {
            map.insert("on_timeout".into(), node_ref(&self.target_id(target)));
        }
        if let Some(target) = &clause.on_reject {
            map.insert("on_reject".into(), node_ref(&self.target_id(target)));
        }
        if !clause.branches.is_empty() {
            let mut branches = Vec::with_capacity(clause.branches.len());
            for edge in &clause.branches {
                let mut branch = Map::new();
                branch.insert("when".into(), self.lower_cond(&edge.when)?);
                branch.insert("target".into(), node_ref(&self.target_id(&edge.target)));
                if let Some(priority) = edge.priority {
                    branch.insert("priority".into(), Value::from(priority));
                }
                branches.push(Value::Object(branch));
            }
            map.insert("branches".into(), Value::Array(branches));
        }
        Ok(Value::Object(map))
    }

    /// the continuation a control block flows into: an explicit forward arrow overrides
    /// the sequential successor.
    fn block_cont(&self, clause: &TransitionClause, cont: &str) -> String {
        match clause.next.as_ref().or(clause.on_success.as_ref()) {
            Some(target) => self.target_id(target),
            None => cont.to_string(),
        }
    }

    fn target_id(&self, target: &Target) -> String {
        match target {
            Target::End => self.end_id.clone(),
            Target::Fail => self.fail_id.clone(),
            Target::Label(name) => name.clone(),
        }
    }

    fn push(&mut self, node: Value) {
        // a node inherits the span of the statement being lowered. synthetic nodes emitted outside
        // any statement (start/end/fail) simply have none.
        if let (Some(span), Some(id)) =
            (self.current_span, node.get("id").and_then(|id| id.as_str()))
        {
            self.spans.push(NodeSpan {
                node_id: id.to_string(),
                start: span.start,
                end: span.end,
            });
        }
        self.nodes.push(node);
    }

    fn claim(&mut self, id: &str) -> Result<String, RexRapError> {
        if !self.used_ids.insert(id.to_string()) {
            return Err(RexRapError::lower(format!("duplicate node id '{id}'")));
        }
        Ok(id.to_string())
    }

    fn fresh_resume(&mut self) -> String {
        loop {
            self.resume_counter += 1;
            let candidate = format!("resume_{}", self.resume_counter);
            if self.used_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }

    fn fresh(&mut self, prefix: &str) -> String {
        loop {
            self.counter += 1;
            let candidate = format!("{prefix}_{}", self.counter);
            if self.used_ids.insert(candidate.clone()) {
                return candidate;
            }
        }
    }
}

fn is_control_stmt(stmt: &Stmt) -> bool {
    matches!(
        &stmt.kind,
        StmtKind::If(_)
            | StmtKind::For(_)
            | StmtKind::While(_)
            | StmtKind::Match(_)
            | StmtKind::Parallel(_)
            | StmtKind::Try(_)
            | StmtKind::Race(_)
            | StmtKind::Map(_)
    )
}

fn is_bound_control_stmt(stmt: &Stmt) -> bool {
    stmt.label.is_some() && is_control_stmt(stmt)
}

fn control_value_expr(kind: &StmtKind) -> Expr {
    if matches!(kind, StmtKind::For(_)) {
        return path_expr(&["prev", "results"]);
    }
    if matches!(kind, StmtKind::Parallel(_)) {
        return Expr::new(
            ExprKind::Object(vec![
                ("branches".into(), path_expr(&["prev", "wait_for"])),
                ("outputs".into(), path_expr(&["prev", "outputs"])),
            ]),
            Span::default(),
        );
    }
    path_expr(&["prev"])
}

fn path_expr(parts: &[&str]) -> Expr {
    Expr::new(
        ExprKind::Path(
            parts
                .iter()
                .map(|part| PathSeg::Key((*part).to_string()))
                .collect(),
        ),
        Span::default(),
    )
}

fn control_prefix(kind: &StmtKind) -> &'static str {
    match kind {
        StmtKind::Action(_) => "action",
        StmtKind::TaskCall(_) => "call",
        StmtKind::Resume(_) => "resume",
        StmtKind::Compute(_) => "compute",
        StmtKind::Subflow(_) => "subflow",
        StmtKind::Wait(_) => "wait",
        StmtKind::Output(_) => "output",
        StmtKind::Yield(_) => "yield",
        StmtKind::Input(_) => "input",
        StmtKind::Approval(_) => "approval",
        StmtKind::Gate(_) => "gate",
        StmtKind::Signal(_) => "signal",
        StmtKind::Assert(_) => "assert",
        StmtKind::Transform(_) => "transform",
        StmtKind::Audit(_) => "audit",
        StmtKind::Checkpoint(_) => "checkpoint",
        StmtKind::Mutex(_) => "mutex",
        StmtKind::Throttle(_) => "throttle",
        StmtKind::Cooldown(_) => "cooldown",
        StmtKind::Await(_) => "await_run",
        StmtKind::Debounce(_) => "debounce",
        StmtKind::Collect(_) => "collect",
        StmtKind::Barrier(_) => "barrier",
        StmtKind::CircuitBreaker(_) => "circuit_breaker",
        StmtKind::EventSource(_) => "event_source",
        StmtKind::Config(_) => "config",
        StmtKind::Return(_) => "return_node",
        StmtKind::Detach(_) => "detach",
        StmtKind::Fail(_) => "fail_node",
        StmtKind::If(_) => "if",
        StmtKind::For(_) => "for_loop",
        StmtKind::While(_) => "while_loop",
        StmtKind::Match(match_stmt) => match match_stmt.mode {
            SwitchMode::Cases => "switch",
            SwitchMode::Toggle => "toggle",
            SwitchMode::Percentage => "percentage",
        },
        StmtKind::Parallel(_) => "parallel",
        StmtKind::Try(_) => "try",
        StmtKind::Race(_) => "race",
        StmtKind::Map(_) => "map",
    }
}

fn provider_actions(
    providers: &[ProviderMetadata],
) -> std::collections::HashMap<(String, String), ActionMetadata> {
    providers
        .iter()
        .flat_map(|provider| {
            provider.actions.iter().map(move |action| {
                (
                    (provider.name.clone(), action.function_name.clone()),
                    action.clone(),
                )
            })
        })
        .collect()
}

// free helpers --------------------------------------------------------------

fn node(id: &str, kind: &str, fields: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    map.insert("id".into(), Value::String(id.to_string()));
    map.insert("kind".into(), Value::String(kind.to_string()));
    for (key, value) in fields {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

fn node_ref(id: &str) -> Value {
    let mut map = Map::new();
    map.insert("$node".into(), Value::String(id.to_string()));
    Value::Object(map)
}

/// whether a type expression references any declared (`type <Name>`) type, anywhere in its shape.
fn type_expr_uses_declared_name(
    ty: &TypeExpr,
    named: &std::collections::BTreeMap<String, runinator_models::types::RuninatorType>,
) -> bool {
    match ty {
        TypeExpr::Named(name) => named.contains_key(name),
        TypeExpr::Task(inner) => inner
            .as_ref()
            .is_some_and(|inner| type_expr_uses_declared_name(inner, named)),
        TypeExpr::Enum(_) => false,
        TypeExpr::Range { base, .. } => type_expr_uses_declared_name(base, named),
        TypeExpr::Array(inner) | TypeExpr::Map(inner) => type_expr_uses_declared_name(inner, named),
        TypeExpr::Union(variants) => variants
            .iter()
            .any(|variant| type_expr_uses_declared_name(variant, named)),
        TypeExpr::Struct { fields, additional } => {
            fields
                .iter()
                .any(|field| type_expr_uses_declared_name(&field.ty, named))
                || additional
                    .as_ref()
                    .is_some_and(|a| type_expr_uses_declared_name(a, named))
        }
        TypeExpr::Function { params, ret } => {
            params
                .iter()
                .any(|param| type_expr_uses_declared_name(param, named))
                || type_expr_uses_declared_name(ret, named)
        }
    }
}

fn transitions_next(target: &str) -> Value {
    let mut map = Map::new();
    map.insert("next".into(), node_ref(target));
    Value::Object(map)
}

/// does every path out of this block already end at a `resume`?
///
/// only the simple shapes are recognised — a trailing `resume`, or an `if`/`match` whose every arm
/// ends in one. anything cleverer falls back to emitting the synthetic terminal, which is the safe
/// direction: an unreachable extra `resume` is harmless, a dangling region path is not.
fn ends_in_resume(block: &[Stmt]) -> bool {
    let Some(last) = block.last() else {
        return false;
    };
    match &last.kind {
        StmtKind::Resume(_) => true,
        StmtKind::If(if_stmt) => {
            if_stmt.arms.iter().all(|arm| ends_in_resume(&arm.1))
                && if_stmt
                    .else_block
                    .as_ref()
                    .is_some_and(|block| ends_in_resume(block))
        }
        _ => false,
    }
}
