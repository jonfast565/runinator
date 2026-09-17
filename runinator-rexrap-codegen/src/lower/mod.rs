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

use runinator_models::orchestration::{
    IngressAction, IngressLifecycle, IngressPolicy, IngressPredicate, IngressPredicateOperator,
    IngressRoute,
};
use runinator_models::providers::{ActionMetadata, ProviderMetadata};
use runinator_models::value::{Map, Value};
use runinator_models::workflows::{WorkflowDefinition, WorkflowGraph};

use runinator_rexrap_sema::CompileOptions;
use runinator_rexrap_sema::desugar::AliasTable;
use runinator_rexrap_sema::types;
use runinator_rexrap_syntax::ast::*;
use runinator_rexrap_syntax::errors::{RexRapError, Span};

/// The runtime target represented by an authored `task[T]` binding. Detached subflows and
/// provider tasks both produce awaitable handles, but their durable identifiers are different.
#[derive(Clone)]
enum TaskBinding {
    Subflow,
    Provider,
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
    let key = workflow.key.as_ref().ok_or_else(|| {
        RexRapError::semantic(
            workflow.span,
            format!("workflow '{}' must declare a stable `key`", workflow.name),
        )
    })?;
    let namespace = workflow.namespace.as_ref().ok_or_else(|| {
        RexRapError::semantic(
            workflow.span,
            format!("workflow '{}' must declare a `namespace`", workflow.name),
        )
    })?;
    let mut lowerer = Lowerer::new();
    let functions = functions_for_workflow(document, workflow);
    lowerer.source_dir = options.source_dir.clone();
    // the callable registry resolves keyword args in both the workflow body and function bodies.
    lowerer.registry = runinator_rexrap_sema::registry::FunctionRegistry::build(&functions);
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
    lowerer.lowered_functions = lowerer.lower_functions(&functions)?;
    // `task fn`s are inlined, not compiled into the function table, so keep them by name.
    for def in &functions {
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
    let imports = workflow
        .imports
        .iter()
        .filter_map(|import| {
            let kind = import.kind?;
            if kind == ImportKind::Module {
                return None;
            }
            let alias = import.alias.as_ref()?;
            let mut entry = Map::new();
            entry.insert("kind".into(), Value::String(kind.keyword().into()));
            entry.insert("path".into(), Value::String(import.path.clone()));
            entry.insert("alias".into(), Value::String(alias.clone()));
            if let Some(revision) = import.revision {
                entry.insert("revision".into(), Value::from(revision));
            }
            Some(Value::Object(entry))
        })
        .collect::<Vec<_>>();
    if !imports.is_empty() {
        rexrap.insert("imports".into(), Value::Array(imports));
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
    if !functions.is_empty() {
        let mut sigs = Map::new();
        for def in &functions {
            sigs.insert(
                def.name.clone(),
                Value::String(runinator_rexrap_syntax::format::format_fn_signature(def)),
            );
        }
        rexrap.insert("functions".into(), Value::Object(sigs));
    }
    let mut source_modules = Vec::new();
    for import in workflow
        .imports
        .iter()
        .filter(|import| import.kind == Some(ImportKind::Module))
    {
        let module = document
            .modules
            .iter()
            .find(|module| module.path == import.path)
            .ok_or_else(|| {
                RexRapError::lower(format!("pack has no source module '{}'", import.path))
            })?;
        let canonical = runinator_rexrap_syntax::format::format_source_module(module);
        let mut entry = Map::new();
        entry.insert("path".into(), Value::String(module.path.clone()));
        entry.insert(
            "digest".into(),
            Value::String(runinator_hash::sha256_digest(canonical.as_bytes())),
        );
        source_modules.push(Value::Object(entry));
    }
    if !source_modules.is_empty() {
        rexrap.insert("source_modules".into(), Value::Array(source_modules));
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
    let mut metadata = match &workflow.metadata {
        Some(expression) => lowerer
            .lower_expr(expression)?
            .as_object()
            .cloned()
            .ok_or_else(|| {
                RexRapError::semantic(expression.span, "workflow metadata must be an object")
            })?,
        None => Map::new(),
    };
    for reserved in [
        "workspace",
        "rexrap",
        "triggers",
        "notifications",
        "concurrency",
        "watches",
        "interrupts",
        "correlation",
        "ingress",
        "functions",
        "artifact_refs",
        "managed_by",
        "namespace",
        "function",
    ] {
        if metadata.contains_key(reserved) {
            return Err(RexRapError::semantic(
                workflow.span,
                format!("workflow metadata key '{reserved}' is reserved"),
            ));
        }
    }
    if let Some(workspace) = &workflow.workspace {
        metadata.insert("workspace".into(), lowerer.lower_expr(workspace)?);
    }
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
    if let Some(ingress) = &workflow.ingress {
        let mut routes = Vec::with_capacity(ingress.routes.len());
        for route in &ingress.routes {
            let lifecycle = match route.lifecycle.as_str() {
                "unbound" => IngressLifecycle::Unbound,
                "active" => IngressLifecycle::Active,
                "terminal" => IngressLifecycle::Terminal,
                _ => {
                    return Err(RexRapError::semantic(
                        route.span,
                        "unknown ingress lifecycle",
                    ));
                }
            };
            let action = match route.action.as_str() {
                "start" => IngressAction::Start,
                "interrupt" => IngressAction::Interrupt,
                "queue" => IngressAction::Queue,
                "record" => IngressAction::Record,
                "requeue" => IngressAction::Requeue,
                "dispatch" => IngressAction::Dispatch,
                _ => return Err(RexRapError::semantic(route.span, "unknown ingress action")),
            };
            if !action.is_allowed_when(lifecycle) {
                return Err(RexRapError::semantic(
                    route.span,
                    "ingress action is not valid for this lifecycle",
                ));
            }
            let predicates = route
                .predicates
                .iter()
                .map(|predicate| {
                    let operator = match predicate.operator.as_str() {
                        "==" => IngressPredicateOperator::Equal,
                        "!=" => IngressPredicateOperator::NotEqual,
                        "in" => IngressPredicateOperator::In,
                        "contains" => IngressPredicateOperator::Contains,
                        "exists" => IngressPredicateOperator::Exists,
                        _ => {
                            return Err(RexRapError::semantic(
                                predicate.span,
                                "unknown ingress predicate operator",
                            ));
                        }
                    };
                    let value = predicate
                        .value
                        .as_ref()
                        .map(|expr| lower_expression_fragment(expr, options))
                        .transpose()?;
                    if value
                        .as_ref()
                        .is_some_and(contains_dynamic_ingress_expression)
                    {
                        return Err(RexRapError::semantic(
                            predicate.span,
                            "ingress predicate values must be literals",
                        ));
                    }
                    Ok(IngressPredicate {
                        pointer: predicate.pointer.clone(),
                        operator,
                        value,
                        resolved_value: None,
                    })
                })
                .collect::<Result<Vec<_>, RexRapError>>()?;
            routes.push(IngressRoute {
                event_type: route.event_type.clone(),
                lifecycle,
                action,
                predicates,
                intent: route.intent.clone(),
            });
        }
        if routes
            .iter()
            .any(|route| route.action == IngressAction::Interrupt)
            && !workflow
                .interrupts
                .iter()
                .any(|handler| handler.enabled && handler.source == "external")
        {
            return Err(RexRapError::semantic(
                ingress.span,
                "ingress interrupt routes require an enabled `interrupt on external` handler",
            ));
        }
        let policy = IngressPolicy {
            scope: ingress.scope.clone(),
            routes,
            setting_bindings: vec![],
        };
        policy
            .validate_dispatches(None)
            .map_err(|message| RexRapError::semantic(ingress.span, message))?;
        metadata.insert(
            "ingress".into(),
            serde_json::to_value(policy)
                .map(Value::from)
                .map_err(|error| RexRapError::lower(error.to_string()))?,
        );
    }
    if !functions.is_empty() {
        metadata.insert("functions".into(), Value::Array(functions));
    }
    if !metadata.is_empty() {
        definition.insert("metadata".into(), Value::Object(metadata));
    }
    if let Some(expression) = &workflow.ui {
        let ui = lowerer.lower_expr(expression)?;
        if !ui.is_object() {
            return Err(RexRapError::semantic(
                expression.span,
                "workflow ui must be an object",
            ));
        }
        definition.insert("ui".into(), ui);
    }
    let graph = WorkflowGraph::from_value(Value::Object(definition)).map_err(RexRapError::lower)?;

    let input_type = match &workflow.input {
        Some(type_expr) => lowerer.lower_input_type(type_expr)?,
        None => Default::default(),
    };

    Ok((
        WorkflowDefinition {
            output_type: workflow
                .output
                .as_ref()
                .map(|ty| lowerer.lower_named_type(ty))
                .transpose()?
                .unwrap_or_default(),
            id: None,
            name: workflow.name.clone(),
            key: Some(key.clone()),
            namespace: Some(namespace.clone()),
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

fn contains_dynamic_ingress_expression(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_dynamic_ingress_expression),
        Value::Object(values) => values
            .iter()
            .any(|(key, value)| key.starts_with('$') || contains_dynamic_ingress_expression(value)),
        _ => false,
    }
}

/// Top-level functions are pack-wide. Source-module functions are embedded only in workflows that
/// explicitly import their module, keeping a compile-time folder from inflating unrelated graphs.
fn functions_for_workflow(document: &Document, workflow: &Workflow) -> Vec<FunctionDef> {
    let imported_modules = workflow
        .imports
        .iter()
        .filter(|import| import.kind == Some(ImportKind::Module))
        .map(|import| import.path.as_str())
        .collect::<HashSet<_>>();
    let mut all_module_functions = HashSet::new();
    let mut imported_module_functions = HashSet::new();
    for module in &document.modules {
        for function in &module.functions {
            let name = module_function_name(&module.path, &function.name);
            all_module_functions.insert(name.clone());
            if imported_modules.contains(module.path.as_str()) {
                imported_module_functions.insert(name);
            }
        }
    }
    document
        .functions
        .iter()
        .filter(|function| {
            !all_module_functions.contains(&function.name)
                || imported_module_functions.contains(&function.name)
        })
        .cloned()
        .collect()
}

fn module_function_name(module: &str, function: &str) -> String {
    let encoded = module
        .split('.')
        .map(|segment| format!("{}{}", segment.len(), segment))
        .collect::<Vec<_>>()
        .join("_");
    format!("__module_{encoded}__{function}")
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

mod var_binding;
use var_binding::VarBinding;

mod lowerer;
use lowerer::Lowerer;

mod lower_entry;
use lower_entry::LowerEntry;

mod node_span;
pub use node_span::NodeSpan;
