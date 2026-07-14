// type checking. seeds an environment from the workflow parameter type and infers expression
// types from there, reusing the `RuninatorType` algebra in runinator-models. only facts the
// front end can know author-time are enforced: parameter field access, iterable `for`/`map`
// sources, orderable comparison operands, and `string()`/`json()` argument kinds. action and
// subflow results, `prev`, and `run` are `Any`, so references through them stay permissive.

use std::collections::{HashMap, HashSet};

use runinator_models::{
    providers::{ActionMetadata, ProviderMetadata},
    types::RuninatorType,
};

use crate::ast::*;
use crate::errors::Span;
use crate::lower::types::{NamedTypes, lower_type_with, resolve_named_types};
use crate::{TypePolicy, WorkflowSignature};

use super::Diagnostic;

/// the typing environment: the workflow parameter type, declared named types, and active loop/map
/// and compute-local variable types.
#[derive(Clone)]
struct Env {
    input: RuninatorType,
    named: NamedTypes,
    node_outputs: HashMap<String, RuninatorType>,
    provider_actions: HashMap<(String, String), ActionMetadata>,
    provider_catalog_present: bool,
    type_policy: TypePolicy,
    workflow_signatures: HashMap<String, WorkflowSignature>,
    scope: Vec<(String, RuninatorType)>,
    // best-effort output type of the source-order predecessor node, used to type `prev`.
    // reset to `Any` at each block boundary, so ambiguous positions (first node, after a
    // control-flow block, inside a nested block) stay opaque. note the first node of a workflow is
    // deliberately not an error: `prev` there is the run's initial/prior payload (see the compute
    // entry-node pattern), which is genuinely dynamic and stays `Any`.
    prev: RuninatorType,
}

pub(super) fn analyze(
    workflow: &Workflow,
    providers: &[ProviderMetadata],
    type_policy: TypePolicy,
    workflow_signatures: &[WorkflowSignature],
    diagnostics: &mut Vec<Diagnostic>,
) {
    report_duplicate_type_decls(workflow, diagnostics);
    // resolve declared type names (ignoring cycle errors, which lowering reports) so
    // parameter and annotation types referencing them type-check against the resolved shape.
    let named = resolve_named_types(&workflow.type_decls).unwrap_or_default();
    let input = workflow
        .input
        .as_ref()
        .and_then(|type_expr| lower_type_with(type_expr, &named).ok())
        .unwrap_or(RuninatorType::Any);
    let provider_actions = provider_actions(providers);
    let workflow_signatures = workflow_signatures
        .iter()
        .cloned()
        .map(|signature| (signature.name.clone(), signature))
        .collect::<HashMap<_, _>>();
    let node_outputs = node_output_types(
        &workflow.body,
        &provider_actions,
        &named,
        &workflow_signatures,
    );
    let mut env = Env {
        input,
        named,
        provider_actions: provider_actions
            .iter()
            .map(|(key, value)| (key.clone(), (*value).clone()))
            .collect(),
        provider_catalog_present: !providers.is_empty(),
        type_policy,
        workflow_signatures,
        scope: Vec::new(),
        node_outputs,
        prev: RuninatorType::Any,
    };
    check_block(&workflow.body, &mut env, diagnostics);
}

fn report_duplicate_type_decls(workflow: &Workflow, diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    for decl in &workflow.type_decls {
        if !seen.insert(decl.name.as_str()) {
            diagnostics.push(Diagnostic::error(
                decl.span,
                format!("duplicate type declaration '{}'", decl.name),
            ));
        }
    }
}

fn provider_actions(
    providers: &[ProviderMetadata],
) -> HashMap<(String, String), &runinator_models::providers::ActionMetadata> {
    providers
        .iter()
        .flat_map(|provider| {
            provider.actions.iter().map(move |action| {
                (
                    (provider.name.clone(), action.function_name.clone()),
                    action,
                )
            })
        })
        .collect()
}

fn node_output_types(
    block: &Block,
    provider_actions: &HashMap<(String, String), &ActionMetadata>,
    named: &NamedTypes,
    workflow_signatures: &HashMap<String, WorkflowSignature>,
) -> HashMap<String, RuninatorType> {
    let mut out = HashMap::new();
    collect_node_output_types(
        block,
        provider_actions,
        named,
        workflow_signatures,
        &mut out,
    );
    out
}

fn collect_node_output_types(
    block: &Block,
    provider_actions: &HashMap<(String, String), &ActionMetadata>,
    named: &NamedTypes,
    workflow_signatures: &HashMap<String, WorkflowSignature>,
    out: &mut HashMap<String, RuninatorType>,
) {
    for stmt in block {
        if let Some(id) = super::effective_id(stmt) {
            let ty = stmt
                .label_type
                .as_ref()
                .and_then(|ty| lower_type_with(ty, named).ok())
                .or_else(|| match &stmt.kind {
                    StmtKind::Action(action) => provider_actions
                        .get(&(action.provider.clone(), action.function.clone()))
                        .filter(|metadata| !metadata.results.is_empty())
                        .map(|metadata| metadata.results_type()),
                    StmtKind::Subflow(subflow) => Some(subflow_output_type(
                        subflow,
                        workflow_signatures.get(&subflow.workflow_name),
                    )),
                    _ => None,
                });
            if let Some(ty) = ty {
                out.insert(id.to_string(), ty);
            }
        }
        for child in super::child_blocks(&stmt.kind) {
            collect_node_output_types(child, provider_actions, named, workflow_signatures, out);
        }
    }
}

fn subflow_output_type(
    subflow: &SubflowStmt,
    signature: Option<&WorkflowSignature>,
) -> RuninatorType {
    // a detached subflow is fire-and-forget: the parent never waits for it, so its output snapshot
    // is never populated here. `state` is `Null` (referencing a field off it is a bug), not `Any`.
    // an awaited subflow takes the callee signature's declared output; `Any` only when unknown.
    let state = if subflow.detached {
        RuninatorType::Null
    } else {
        signature
            .map(|signature| signature.output.clone())
            .unwrap_or(RuninatorType::Any)
    };
    // the echoed-back `parameters` are exactly what we passed in: the callee signature's input.
    let parameters = signature
        .map(|signature| signature.input.clone())
        .unwrap_or(RuninatorType::Any);
    RuninatorType::structure([
        ("subflow_run_id", RuninatorType::String),
        ("subflow_workflow_id", RuninatorType::String),
        ("run_name", RuninatorType::String),
        ("reused", RuninatorType::Boolean),
        ("status", RuninatorType::String),
        ("state", state),
        ("parameters", parameters),
    ])
}

fn check_block(block: &Block, env: &mut Env, diagnostics: &mut Vec<Diagnostic>) {
    // a block starts with no known predecessor; `prev` becomes concrete only after a straight-line
    // producing node (action/subflow/typed label) runs earlier in the same block.
    env.prev = RuninatorType::Any;
    for stmt in block {
        check_stmt(stmt, env, diagnostics);
        env.prev = predecessor_output(stmt, env);
    }
}

/// the best-effort output type a straight-line successor would see as `prev`. reuses the
/// precomputed `node_outputs` map, so only producing nodes (action/subflow/typed label) yield a
/// concrete type; control-flow and effect-free nodes fall back to `Any`.
fn predecessor_output(stmt: &Stmt, env: &Env) -> RuninatorType {
    super::effective_id(stmt)
        .and_then(|id| env.node_outputs.get(id).cloned())
        .unwrap_or(RuninatorType::Any)
}

fn check_stmt(stmt: &Stmt, env: &mut Env, diagnostics: &mut Vec<Diagnostic>) {
    check_label_type(stmt, env, diagnostics);
    match &stmt.kind {
        StmtKind::Action(action) => {
            check_action(action, stmt.span, env, diagnostics);
            for (_, value) in &action.args {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Compute(compute) => {
            let base = env.scope.len();
            check_compute_block(&compute.body, env, diagnostics);
            env.scope.truncate(base);
        }
        StmtKind::Subflow(subflow) => {
            check_subflow(subflow, stmt.span, env, diagnostics);
            if let Some(run_name) = &subflow.run_name {
                check_expr(run_name, env, diagnostics);
            }
            for (_, value) in &subflow.params {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Wait(_) => {}
        StmtKind::Output(output) => {
            if let Some(data) = &output.data {
                check_expr(data, env, diagnostics);
            }
            for (_, source) in &output.items {
                check_expr(source, env, diagnostics);
            }
        }
        StmtKind::Yield(value) => check_expr(value, env, diagnostics),
        StmtKind::Input(input) => {
            if let Some(prompt) = &input.prompt {
                check_expr(prompt, env, diagnostics);
            }
        }
        StmtKind::Approval(approval) => {
            check_expr(&approval.prompt, env, diagnostics);
            for (_, value) in &approval.metadata {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Gate(gate) => {
            if let Some(when) = &gate.when {
                check_cond(when, env, diagnostics);
            }
            for (_, value) in &gate.metadata {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Signal(signal) => {
            for (_, value) in &signal.metadata {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Config(config) => {
            if let Some(name) = &config.name {
                check_expr(name, env, diagnostics);
            }
            if let Some(metadata) = &config.metadata {
                check_expr(metadata, env, diagnostics);
            }
        }
        StmtKind::Fail(message) => {
            if let Some(message) = message {
                check_expr(message, env, diagnostics);
            }
        }
        StmtKind::If(if_stmt) => {
            for (cond, body) in &if_stmt.arms {
                check_cond(cond, env, diagnostics);
                check_block(body, env, diagnostics);
            }
            if let Some(else_block) = &if_stmt.else_block {
                check_block(else_block, env, diagnostics);
            }
        }
        StmtKind::For(for_stmt) => {
            let element = check_iterable(&for_stmt.items, env, "for loop", diagnostics);
            env.scope.push((for_stmt.var.clone(), element));
            check_block(&for_stmt.body, env, diagnostics);
            env.scope.pop();
        }
        StmtKind::While(while_stmt) => {
            check_cond(&while_stmt.cond, env, diagnostics);
            check_block(&while_stmt.body, env, diagnostics);
        }
        StmtKind::Map(map_stmt) => {
            let element = check_iterable(&map_stmt.items, env, "map", diagnostics);
            env.scope.push((map_stmt.var.clone(), element));
            check_block(&map_stmt.body, env, diagnostics);
            env.scope.pop();
        }
        StmtKind::Match(match_stmt) => {
            check_expr(&match_stmt.subject, env, diagnostics);
            for arm in &match_stmt.arms {
                if let Some(equals) = &arm.equals {
                    check_expr(equals, env, diagnostics);
                }
                if let Some(when) = &arm.when {
                    check_cond(when, env, diagnostics);
                }
                check_block(&arm.body, env, diagnostics);
            }
            if let Some(default) = &match_stmt.default {
                check_block(default, env, diagnostics);
            }
        }
        StmtKind::Parallel(parallel) => {
            for branch in &parallel.branches {
                check_block(branch, env, diagnostics);
            }
        }
        StmtKind::Race(race) => {
            for branch in &race.branches {
                check_block(branch, env, diagnostics);
            }
        }
        StmtKind::Try(try_stmt) => {
            check_block(&try_stmt.body, env, diagnostics);
            if let Some(catch) = &try_stmt.catch {
                check_block(catch, env, diagnostics);
            }
            if let Some(finally) = &try_stmt.finally {
                check_block(finally, env, diagnostics);
            }
        }
        StmtKind::Assert(assert) => {
            for (_, cond) in &assert.assertions {
                check_cond(cond, env, diagnostics);
            }
        }
        StmtKind::Transform(transform) => {
            for (_, value) in &transform.bindings {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Audit(audit) => {
            check_expr(&audit.action, env, diagnostics);
            for value in [
                audit.actor.as_ref(),
                audit.target.as_ref(),
                audit.reason.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                check_expr(value, env, diagnostics);
            }
        }
        StmtKind::Await(await_stmt) => check_expr(&await_stmt.run_ids, env, diagnostics),
        StmtKind::Debounce(debounce) => {
            if let Some(key) = &debounce.key {
                check_expr(key, env, diagnostics);
            }
        }
        StmtKind::EventSource(es) => {
            if let Some(filter) = &es.filter {
                check_cond(filter, env, diagnostics);
            }
        }
        StmtKind::Mutex(mutex) => check_block(&mutex.body, env, diagnostics),
        // these carry no expressions to type-check.
        StmtKind::Checkpoint(_)
        | StmtKind::Throttle(_)
        | StmtKind::Collect(_)
        | StmtKind::Barrier(_)
        | StmtKind::CircuitBreaker(_) => {}
    }
}

fn check_label_type(stmt: &Stmt, env: &Env, diagnostics: &mut Vec<Diagnostic>) {
    let Some(label_type) = &stmt.label_type else {
        return;
    };
    let declared = lower_type_with(label_type, &env.named).unwrap_or(RuninatorType::Any);
    let inferred = match &stmt.kind {
        StmtKind::Action(action) => env
            .provider_actions
            .get(&(action.provider.clone(), action.function.clone()))
            .map(ActionMetadata::results_type),
        StmtKind::Subflow(subflow) => Some(subflow_output_type(
            subflow,
            env.workflow_signatures.get(&subflow.workflow_name),
        )),
        _ => None,
    };
    if let Some(inferred) = inferred {
        check_assignable(
            &inferred,
            &declared,
            "node output annotation",
            stmt.span,
            diagnostics,
        );
    }
}

fn check_action(action: &ActionStmt, span: Span, env: &Env, diagnostics: &mut Vec<Diagnostic>) {
    let key = (action.provider.clone(), action.function.clone());
    let Some(metadata) = env.provider_actions.get(&key) else {
        if env.provider_catalog_present && env.type_policy == TypePolicy::Strict {
            diagnostics.push(Diagnostic::error(
                span,
                format!(
                    "unknown provider action '{}.{}'",
                    action.provider, action.function
                ),
            ));
        }
        return;
    };

    let params = metadata
        .parameters
        .iter()
        .map(|param| (param.name.as_str(), param))
        .collect::<HashMap<_, _>>();
    for param in &metadata.parameters {
        if param.required && action.args.iter().all(|(name, _)| name != &param.name) {
            diagnostics.push(Diagnostic::error(
                span,
                format!(
                    "action '{}.{}' is missing required parameter '{}'",
                    action.provider, action.function, param.name
                ),
            ));
        }
    }
    for (name, value) in &action.args {
        let Some(param) = params.get(name.as_str()) else {
            diagnostics.push(Diagnostic::error(
                value.span,
                format!(
                    "unknown parameter '{}' for action '{}.{}'",
                    name, action.provider, action.function
                ),
            ));
            continue;
        };
        let actual = infer_expr(value, env, diagnostics);
        check_assignable(
            &actual,
            &param.ty,
            &format!("action parameter '{}'", param.name),
            value.span,
            diagnostics,
        );
    }
}

fn check_subflow(subflow: &SubflowStmt, span: Span, env: &Env, diagnostics: &mut Vec<Diagnostic>) {
    let Some(signature) = env.workflow_signatures.get(&subflow.workflow_name) else {
        if env.type_policy == TypePolicy::Strict {
            diagnostics.push(Diagnostic::error(
                span,
                format!("unknown subflow target '{}'", subflow.workflow_name),
            ));
        }
        return;
    };
    let actual = RuninatorType::structure(
        subflow
            .params
            .iter()
            .map(|(name, value)| (name.clone(), infer_expr(value, env, diagnostics))),
    );
    check_assignable(
        &actual,
        &signature.input,
        &format!("subflow '{}' parameters", subflow.workflow_name),
        span,
        diagnostics,
    );
}

/// require an iterable source and return its element type (`Any` when unknown).
fn check_iterable(
    items: &Expr,
    env: &Env,
    label: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    let ty = infer_expr(items, env, diagnostics);
    match &ty {
        RuninatorType::Union(_) => ty.union_element_type().unwrap_or(RuninatorType::Any),
        other => other.element_type().unwrap_or_else(|| {
            diagnostics.push(Diagnostic::error(
                items.span,
                format!("{label} expects an array, got {}", other.describe()),
            ));
            RuninatorType::Any
        }),
    }
}

fn check_cond(cond: &Cond, env: &Env, diagnostics: &mut Vec<Diagnostic>) {
    match &cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            for part in parts {
                check_cond(part, env, diagnostics);
            }
        }
        CondKind::Not(inner) => check_cond(inner, env, diagnostics),
        CondKind::Expr(expr) => {
            let ty = infer_expr(expr, env, diagnostics);
            check_assignable(
                &ty,
                &RuninatorType::Boolean,
                "condition",
                expr.span,
                diagnostics,
            );
        }
        CondKind::Exists(expr) => check_expr(expr, env, diagnostics),
        CondKind::Cmp { left, op, right } => {
            let left_ty = infer_expr(left, env, diagnostics);
            let right_ty = infer_expr(right, env, diagnostics);
            match op {
                CmpOp::Gt | CmpOp::Ge | CmpOp::Lt | CmpOp::Le => {
                    require_orderable(&left_ty, left.span, diagnostics);
                    require_orderable(&right_ty, right.span, diagnostics);
                }
                CmpOp::StartsWith | CmpOp::EndsWith => {
                    require_stringish(&left_ty, left.span, diagnostics);
                    require_stringish(&right_ty, right.span, diagnostics);
                }
                _ => {}
            }
        }
    }
}

fn check_expr(expr: &Expr, env: &Env, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        ExprKind::ToString(inner) => {
            let ty = infer_expr(inner, env, diagnostics);
            if is_composite(&ty) {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!("string() expects a scalar, got {}", ty.describe()),
                ));
            }
        }
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    check_expr(inner, env, diagnostics);
                }
            }
        }
        ExprKind::Array(items) => {
            for item in items {
                check_expr(item, env, diagnostics);
            }
        }
        ExprKind::Object(entries) => {
            for (_, value) in entries {
                check_expr(value, env, diagnostics);
            }
        }
        ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
            for part in parts {
                check_expr(part, env, diagnostics);
            }
        }
        ExprKind::ToJson(inner) => {
            let ty = infer_expr(inner, env, diagnostics);
            if !matches!(
                ty,
                RuninatorType::Array(_)
                    | RuninatorType::Map(_)
                    | RuninatorType::Struct { .. }
                    | RuninatorType::Any
                    | RuninatorType::Union(_)
            ) {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!("json() expects a composite value, got {}", ty.describe()),
                ));
            }
        }
        ExprKind::Neg(inner) => {
            let ty = infer_expr(inner, env, diagnostics);
            require_numeric(&ty, inner.span, diagnostics);
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts {
                let ty = infer_expr(part, env, diagnostics);
                require_numeric(&ty, part.span, diagnostics);
            }
        }
        ExprKind::Compare { left, right, .. } => {
            check_expr(left, env, diagnostics);
            check_expr(right, env, diagnostics);
        }
        ExprKind::Ternary { cond, then, els } => {
            let cond_ty = infer_expr(cond, env, diagnostics);
            check_assignable(
                &cond_ty,
                &RuninatorType::Boolean,
                "ternary condition",
                cond.span,
                diagnostics,
            );
            // disjoint branches are not an error: they unify to a union (see `infer_expr`).
            check_expr(then, env, diagnostics);
            check_expr(els, env, diagnostics);
        }
        ExprKind::Call {
            name, args, named, ..
        } => {
            if runinator_workflows::is_higher_order(name) {
                let _ = infer_higher_order_call_type(name, args, env, expr.span, diagnostics);
                return;
            }
            // a call to a local bound to a first-class lambda: check argument count (its parameter
            // types are unconstrained `any`, so only arity is enforced).
            if let Some(RuninatorType::Function { params, .. }) = function_local(name, env) {
                if args.len() != params.len() {
                    diagnostics.push(Diagnostic::error(
                        expr.span,
                        format!(
                            "'{name}' expects {} argument(s), got {}",
                            params.len(),
                            args.len()
                        ),
                    ));
                }
                for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                    check_expr(arg, env, diagnostics);
                }
                return;
            }
            // check each positional argument against the intrinsic's declared parameter type,
            // skipping opaque (`any`) types on either side to avoid false positives on refs.
            if let Some(sig) = runinator_workflows::intrinsic_signature(name) {
                for (param, arg) in sig.parameters.iter().zip(args.iter()) {
                    let arg_ty = infer_expr(arg, env, diagnostics);
                    check_assignable(
                        &arg_ty,
                        &param.ty,
                        &format!("intrinsic '{name}' argument '{}'", param.name),
                        arg.span,
                        diagnostics,
                    );
                }
            }
            for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                check_expr(arg, env, diagnostics);
            }
        }
        // a lambda body is checked permissively; its params type as `Any` (unknown reference heads
        // stay opaque), so no spurious diagnostics arise from the bound names.
        ExprKind::Lambda { body, .. } => check_expr(body, env, diagnostics),
        // a cast asserts the inner value has the target type: check the inner expression, then that
        // its inferred type is assignable to the target (opaque `any` values pass, which is the
        // point — `parse_json(s) as T` and `[] as T[]` adopt `T` here).
        ExprKind::Cast { expr: inner, ty } => {
            check_expr(inner, env, diagnostics);
            let declared = lower_type_with(ty, &env.named).unwrap_or(RuninatorType::Any);
            let actual = infer_expr(inner, env, diagnostics);
            check_assignable(&actual, &declared, "cast", expr.span, diagnostics);
        }
        // applying a callee value: check the callee and arguments, then that the callee is a function
        // of matching arity. an opaque (`any`) callee stays permissive.
        ExprKind::Apply { callee, args } => {
            check_expr(callee, env, diagnostics);
            for arg in args {
                check_expr(arg, env, diagnostics);
            }
            match infer_expr(callee, env, diagnostics) {
                RuninatorType::Function { params, .. } if params.len() != args.len() => {
                    diagnostics.push(Diagnostic::error(
                        expr.span,
                        format!(
                            "applied function expects {} argument(s), got {}",
                            params.len(),
                            args.len()
                        ),
                    ));
                }
                RuninatorType::Function { .. } | RuninatorType::Any => {}
                other => diagnostics.push(Diagnostic::error(
                    callee.span,
                    format!("cannot apply a value of type {}", other.describe()),
                )),
            }
        }
        // paths drive field-access diagnostics through inference.
        ExprKind::Path(_) => {
            let _ = infer_expr(expr, env, diagnostics);
        }
        // spreads are expanded before sema runs; nothing to check.
        ExprKind::Spread(_) => {}
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::FileInclude { .. }
        | ExprKind::DirInclude { .. }
        | ExprKind::InlineCode { .. } => {}
    }
}

/// type-check a compute block: thread typed locals through `let` (so later lines see them), check
/// each `let x: T` value against its annotation, and recurse into nested `if` branches with block
/// scoping.
fn check_compute_block(
    body: &[crate::ast::ComputeLine],
    env: &mut Env,
    diagnostics: &mut Vec<Diagnostic>,
) {
    use crate::ast::ComputeLine;
    for line in body {
        match line {
            ComputeLine::Let { name, ty, value } => {
                check_expr(value, env, diagnostics);
                let declared = ty
                    .as_ref()
                    .map(|t| lower_type_with(t, &env.named).unwrap_or(RuninatorType::Any));
                // for `let f: function<(A) -> R> = <lambda>`, bind the lambda's parameters to the
                // declared parameter types so the body checks against the annotation (bidirectional),
                // rather than typing every parameter `Any`.
                let value_ty = match (&declared, &value.kind) {
                    (
                        Some(RuninatorType::Function {
                            params: expected, ..
                        }),
                        ExprKind::Lambda { params, body },
                    ) if params.len() == expected.len() => {
                        let mut scoped = env.clone();
                        for (param, param_ty) in params.iter().zip(expected.iter()) {
                            scoped.scope.push((param.clone(), param_ty.clone()));
                        }
                        let ret = infer_expr(body, &scoped, diagnostics);
                        RuninatorType::Function {
                            params: expected.clone(),
                            ret: Box::new(ret),
                        }
                    }
                    _ => infer_expr(value, env, diagnostics),
                };
                if let Some(declared) = &declared {
                    check_assignable(
                        &value_ty,
                        declared,
                        &format!("compute local '{name}'"),
                        value.span,
                        diagnostics,
                    );
                }
                // a later reference to the local sees its declared type, or the inferred one.
                let local_ty = declared.unwrap_or(value_ty);
                env.scope.push((name.clone(), local_ty));
            }
            ComputeLine::Return(value) | ComputeLine::Expr(value) => {
                check_expr(value, env, diagnostics)
            }
            ComputeLine::If {
                then_branch,
                else_branch,
                ..
            } => {
                let base = env.scope.len();
                check_compute_block(then_branch, env, diagnostics);
                env.scope.truncate(base);
                check_compute_block(else_branch, env, diagnostics);
                env.scope.truncate(base);
            }
            ComputeLine::Goto(_) => {}
        }
    }
}

/// report a type error when `actual` cannot be assigned to `expected`. opaque (`any`) types on
/// either side are accepted so author-time-unknown values (prev/node references) stay permissive.
fn check_assignable(
    actual: &RuninatorType,
    expected: &RuninatorType,
    label: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if matches!(actual, RuninatorType::Any) || matches!(expected, RuninatorType::Any) {
        return;
    }
    if let Err(violation) = validate_author_assignable(actual, expected) {
        diagnostics.push(Diagnostic::error(span, violation.message_with_label(label)));
    }
}

fn validate_author_assignable(
    actual: &RuninatorType,
    expected: &RuninatorType,
) -> Result<(), runinator_models::types::TypeViolation> {
    if matches!(actual, RuninatorType::Any) || matches!(expected, RuninatorType::Any) {
        return Ok(());
    }
    match (actual, expected) {
        (
            RuninatorType::Struct {
                fields: actual_fields,
                additional: actual_additional,
            },
            RuninatorType::Struct {
                fields: expected_fields,
                additional: expected_additional,
            },
        ) => {
            for (key, expected_field) in expected_fields {
                let Some(actual_field) = actual_fields.get(key) else {
                    if expected_field.required {
                        return actual.validate_assignable_to(expected);
                    }
                    continue;
                };
                validate_author_assignable(&actual_field.ty, &expected_field.ty)?;
            }
            for (key, actual_field) in actual_fields {
                if expected_fields.contains_key(key) {
                    continue;
                }
                let Some(expected_additional) = expected_additional else {
                    return actual.validate_assignable_to(expected);
                };
                validate_author_assignable(&actual_field.ty, expected_additional)?;
            }
            if let (Some(actual_additional), Some(expected_additional)) =
                (actual_additional, expected_additional)
            {
                validate_author_assignable(actual_additional, expected_additional)?;
            }
            Ok(())
        }
        (RuninatorType::Array(actual), RuninatorType::Array(expected))
        | (RuninatorType::Map(actual), RuninatorType::Map(expected)) => {
            validate_author_assignable(actual, expected)
        }
        _ => actual.validate_assignable_to(expected),
    }
}

fn infer_expr(expr: &Expr, env: &Env, diagnostics: &mut Vec<Diagnostic>) -> RuninatorType {
    match &expr.kind {
        ExprKind::Null => RuninatorType::Null,
        ExprKind::Bool(_) => RuninatorType::Boolean,
        ExprKind::Int(_) => RuninatorType::Integer,
        ExprKind::Float(_) => RuninatorType::Number,
        ExprKind::Str(_) => RuninatorType::String,
        ExprKind::FileInclude { .. } => RuninatorType::String,
        ExprKind::DirInclude { .. } => RuninatorType::array(RuninatorType::String),
        ExprKind::InlineCode { .. } => RuninatorType::String,
        ExprKind::Concat(_) => RuninatorType::String,
        ExprKind::ToString(_) => RuninatorType::String,
        ExprKind::ToJson(_) => RuninatorType::String,
        ExprKind::Coalesce(parts) => {
            let mut resolved: Option<RuninatorType> = None;
            for part in parts {
                let ty = infer_expr(part, env, diagnostics);
                if ty == RuninatorType::Null {
                    continue;
                }
                resolved = Some(match resolved {
                    None => ty,
                    Some(existing) => existing.unify(&ty),
                });
            }
            resolved.unwrap_or(RuninatorType::Null)
        }
        ExprKind::Array(items) => {
            let mut element: Option<RuninatorType> = None;
            for item in items {
                let item_ty = infer_expr(item, env, diagnostics);
                match &element {
                    None => element = Some(item_ty),
                    Some(existing) => {
                        if let Some(common) = common_type(existing, &item_ty) {
                            element = Some(common);
                        } else {
                            if env.type_policy == TypePolicy::Strict {
                                diagnostics.push(Diagnostic::error(
                                    item.span,
                                    format!(
                                        "array item type {} is incompatible with {}",
                                        item_ty.describe(),
                                        existing.describe()
                                    ),
                                ));
                            }
                            return RuninatorType::array(RuninatorType::Any);
                        }
                    }
                }
            }
            RuninatorType::array(element.unwrap_or(RuninatorType::Any))
        }
        ExprKind::Object(entries) => RuninatorType::structure(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), infer_expr(value, env, diagnostics))),
        ),
        ExprKind::Path(segs) => infer_path(segs, env, expr.span, diagnostics),
        // arithmetic yields a number; intrinsic call results are author-time opaque.
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => numeric_result_type(parts, env, diagnostics),
        ExprKind::Neg(inner) => {
            let ty = infer_expr(inner, env, diagnostics);
            if ty == RuninatorType::Integer {
                RuninatorType::Integer
            } else {
                RuninatorType::Number
            }
        }
        // a relational comparison resolves to a boolean.
        ExprKind::Compare { .. } => RuninatorType::Boolean,
        // a ternary resolves to its branches' common type, or a union when they differ.
        ExprKind::Ternary { then, els, .. } => {
            let then_ty = infer_expr(then, env, diagnostics);
            let els_ty = infer_expr(els, env, diagnostics);
            then_ty.unify(&els_ty)
        }
        // a call's result type comes from the intrinsic signature or, for higher-order intrinsics,
        // from the collection and lambda argument types.
        ExprKind::Call { name, args, .. } => {
            if runinator_workflows::is_higher_order(name) {
                infer_higher_order_call_type(name, args, env, expr.span, diagnostics)
            } else if let Some(RuninatorType::Function { ret, .. }) = function_local(name, env) {
                // a call to a local bound to a first-class lambda yields the function's result type.
                (**ret).clone()
            } else {
                // infer argument types into a throwaway sink (the check pass already validates the
                // arguments) so the polymorphic intrinsics recover an argument-dependent result
                // type before falling back to the catalog's declared (often `any`) result.
                let mut sink = Vec::new();
                let arg_types = args
                    .iter()
                    .map(|arg| infer_expr(arg, env, &mut sink))
                    .collect::<Vec<_>>();
                // extract any literal key(s) from the second argument for key-driven intrinsics.
                let literal_keys = args.get(1).and_then(crate::ast::static_string_keys);
                runinator_workflows::intrinsic_result_type(
                    name,
                    &arg_types,
                    literal_keys.as_deref(),
                )
                .or_else(|| {
                    runinator_workflows::intrinsic_signature(name)
                        .and_then(|sig| sig.results.first().map(|result| result.ty.clone()))
                })
                .unwrap_or(RuninatorType::Any)
            }
        }
        // a lambda's value type: its parameters are unconstrained in a value position (`Any`) and
        // its result is the body's type with the parameters in scope.
        ExprKind::Lambda { params, body } => {
            let mut scoped = env.clone();
            for param in params {
                scoped.scope.push((param.clone(), RuninatorType::Any));
            }
            let ret = infer_expr(body, &scoped, diagnostics);
            RuninatorType::Function {
                params: params.iter().map(|_| RuninatorType::Any).collect(),
                ret: Box::new(ret),
            }
        }
        // a cast's inferred type is exactly the asserted target: this is what lets an opaque inner
        // value (`parse_json`, `[]`) resolve to a concrete shape at the cast position.
        ExprKind::Cast { ty, .. } => lower_type_with(ty, &env.named).unwrap_or(RuninatorType::Any),
        // applying a callee value yields the callee function's result type, or `Any` when the callee
        // is opaque or not a function (the check pass reports a genuine non-function application).
        ExprKind::Apply { callee, .. } => match infer_expr(callee, env, diagnostics) {
            RuninatorType::Function { ret, .. } => *ret,
            _ => RuninatorType::Any,
        },
        // spreads are expanded before sema runs; treat as untyped if one is reached.
        ExprKind::Spread(_) => RuninatorType::Any,
    }
}

fn infer_higher_order_call_type(
    name: &str,
    args: &[Expr],
    env: &Env,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    let Some(collection) = args.first() else {
        diagnostics.push(Diagnostic::error(
            span,
            format!("'{name}' is missing a collection argument"),
        ));
        return RuninatorType::Any;
    };
    let collection_type = infer_expr(collection, env, diagnostics);
    let item_type = collection_item_type(name, &collection_type, collection.span, diagnostics);
    match name {
        "map" => {
            let body_type =
                infer_lambda_type(name, args.get(1), &[(0, item_type)], env, span, diagnostics);
            RuninatorType::array(body_type)
        }
        "flat_map" => {
            let body_type =
                infer_lambda_type(name, args.get(1), &[(0, item_type)], env, span, diagnostics);
            match body_type {
                RuninatorType::Array(inner) => RuninatorType::array(*inner),
                other => RuninatorType::array(other),
            }
        }
        "filter" => {
            let body_type = infer_lambda_type(
                name,
                args.get(1),
                &[(0, item_type.clone())],
                env,
                span,
                diagnostics,
            );
            check_assignable(
                &body_type,
                &RuninatorType::Boolean,
                "'filter' lambda",
                args.get(1).map(|arg| arg.span).unwrap_or(span),
                diagnostics,
            );
            RuninatorType::array(item_type)
        }
        "find" => {
            let body_type = infer_lambda_type(
                name,
                args.get(1),
                &[(0, item_type.clone())],
                env,
                span,
                diagnostics,
            );
            check_assignable(
                &body_type,
                &RuninatorType::Boolean,
                "'find' lambda",
                args.get(1).map(|arg| arg.span).unwrap_or(span),
                diagnostics,
            );
            RuninatorType::Union(vec![item_type, RuninatorType::Null])
        }
        "any" | "all" => {
            let body_type =
                infer_lambda_type(name, args.get(1), &[(0, item_type)], env, span, diagnostics);
            check_assignable(
                &body_type,
                &RuninatorType::Boolean,
                &format!("'{name}' lambda"),
                args.get(1).map(|arg| arg.span).unwrap_or(span),
                diagnostics,
            );
            RuninatorType::Boolean
        }
        "sort_by" => {
            let body_type = infer_lambda_type(
                name,
                args.get(1),
                &[(0, item_type.clone())],
                env,
                span,
                diagnostics,
            );
            require_orderable(
                &body_type,
                args.get(1).map(|arg| arg.span).unwrap_or(span),
                diagnostics,
            );
            RuninatorType::array(item_type)
        }
        "reduce" => {
            let accumulator_type = args
                .get(1)
                .map(|arg| infer_expr(arg, env, diagnostics))
                .unwrap_or_else(|| {
                    diagnostics.push(Diagnostic::error(
                        span,
                        "'reduce' is missing an initial accumulator argument",
                    ));
                    RuninatorType::Any
                });
            let body_type = infer_lambda_type(
                name,
                args.get(2),
                &[(0, accumulator_type.clone()), (1, item_type)],
                env,
                span,
                diagnostics,
            );
            if let Some(result_type) = common_type(&accumulator_type, &body_type) {
                return result_type;
            }
            check_assignable(
                &body_type,
                &accumulator_type,
                "'reduce' lambda",
                args.get(2).map(|arg| arg.span).unwrap_or(span),
                diagnostics,
            );
            accumulator_type
        }
        _ => RuninatorType::Any,
    }
}

fn collection_item_type(
    name: &str,
    ty: &RuninatorType,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    match ty {
        RuninatorType::Union(_) => ty.union_element_type().unwrap_or(RuninatorType::Any),
        other => other.element_type().unwrap_or_else(|| {
            diagnostics.push(Diagnostic::error(
                span,
                format!("'{name}' expects an array, got {}", other.describe()),
            ));
            RuninatorType::Any
        }),
    }
}

fn common_type(left: &RuninatorType, right: &RuninatorType) -> Option<RuninatorType> {
    left.common_type(right)
}

fn numeric_result_type(
    parts: &[Expr],
    env: &Env,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    let mut all_integer = true;
    for part in parts {
        let ty = infer_expr(part, env, diagnostics);
        if !matches!(ty, RuninatorType::Integer | RuninatorType::Duration) {
            all_integer = false;
        }
    }
    if all_integer {
        RuninatorType::Integer
    } else {
        RuninatorType::Number
    }
}

/// find a scope-local bound to a first-class function type (a lambda value), if any.
fn function_local<'a>(name: &str, env: &'a Env) -> Option<&'a RuninatorType> {
    env.scope
        .iter()
        .rev()
        .find(|(local, ty)| local == name && matches!(ty, RuninatorType::Function { .. }))
        .map(|(_, ty)| ty)
}

fn infer_lambda_type(
    name: &str,
    expr: Option<&Expr>,
    bindings: &[(usize, RuninatorType)],
    env: &Env,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    let Some(expr) = expr else {
        diagnostics.push(Diagnostic::error(
            span,
            format!("'{name}' is missing a lambda argument"),
        ));
        return RuninatorType::Any;
    };
    let ExprKind::Lambda { params, body } = &expr.kind else {
        // a first-class function value passed by reference: use its declared result type when the
        // arity matches; stay permissive for an opaque (`any`) reference.
        let ty = infer_expr(expr, env, diagnostics);
        return match ty {
            RuninatorType::Function {
                params: fn_params,
                ret,
            } => {
                if fn_params.len() != bindings.len() {
                    diagnostics.push(Diagnostic::error(
                        expr.span,
                        format!(
                            "'{name}' expects a {}-parameter function, got {}",
                            bindings.len(),
                            fn_params.len()
                        ),
                    ));
                    return RuninatorType::Any;
                }
                *ret
            }
            RuninatorType::Any => RuninatorType::Any,
            _ => {
                diagnostics.push(Diagnostic::error(
                    expr.span,
                    format!("'{name}' requires a lambda argument"),
                ));
                RuninatorType::Any
            }
        };
    };
    let required = bindings.len();
    if params.len() != required {
        diagnostics.push(Diagnostic::error(
            expr.span,
            format!(
                "'{name}' lambda expects {required} parameter(s), got {}",
                params.len()
            ),
        ));
        return RuninatorType::Any;
    }
    let mut scoped = env.clone();
    for (index, ty) in bindings {
        scoped.scope.push((params[*index].clone(), ty.clone()));
    }
    check_expr(body, &scoped, diagnostics);
    infer_expr(body, &scoped, diagnostics)
}

fn infer_path(
    segs: &[PathSeg],
    env: &Env,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    let Some(PathSeg::Key(head)) = segs.first() else {
        return RuninatorType::Any;
    };
    let rest = &segs[1..];
    // a loop/map variable shadows everything else; params and typed node outputs follow.
    if let Some((_, ty)) = env.scope.iter().rev().find(|(name, _)| name == head) {
        return navigate(ty.clone(), rest, head, span, diagnostics);
    }
    if head == "params" {
        return navigate(env.input.clone(), rest, head, span, diagnostics);
    }
    if let Some(ty) = env.node_outputs.get(head) {
        return navigate(ty.clone(), rest, head, span, diagnostics);
    }
    // `prev` resolves to the source-order predecessor's output type when it is a producing node;
    // it is `Any` at ambiguous positions (first node, after control flow, nested blocks).
    if head == "prev" {
        return navigate(env.prev.clone(), rest, head, span, diagnostics);
    }
    // `run` exposes the current run's metadata; its shape is single-sourced from the runtime header.
    if head == "run" {
        let run_type = runinator_models::workflow_state::WorkflowContextHeader::runinator_type();
        return navigate(run_type, rest, head, span, diagnostics);
    }
    // a bare node reference with no recorded output shape is opaque author-time.
    RuninatorType::Any
}

/// walk a dotted path through a known type, reporting missing fields on closed structs.
fn navigate(
    mut ty: RuninatorType,
    segs: &[PathSeg],
    root: &str,
    span: Span,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    for (index, seg) in segs.iter().enumerate() {
        // descending through an opaque `Any` cannot be narrowed further.
        if matches!(ty, RuninatorType::Any) {
            return RuninatorType::Any;
        }
        // a union navigates the rest of the path on each variant and re-unions the results, so a
        // segment valid on every variant keeps a concrete type instead of collapsing to `Any`.
        // per-variant diagnostics are suppressed (a field may legitimately exist on only some
        // variants); an unresolvable segment yields `Any` via `union_element_type`-style widening.
        if let RuninatorType::Union(variants) = &ty {
            let rest = &segs[index..];
            let mut resolved: Option<RuninatorType> = None;
            for variant in variants {
                let navigated = navigate(variant.clone(), rest, root, span, &mut Vec::new());
                resolved = Some(match resolved {
                    None => navigated,
                    Some(existing) => existing.unify(&navigated),
                });
            }
            return resolved.unwrap_or(RuninatorType::Any);
        }
        match seg {
            PathSeg::Key(key) => match ty {
                RuninatorType::Struct { fields, additional } => {
                    if let Some(field) = fields.get(key) {
                        ty = field.ty.clone();
                    } else if let Some(extra) = &additional {
                        ty = (**extra).clone();
                    } else {
                        diagnostics.push(Diagnostic::error(
                            span,
                            format!("unknown field '{key}' on '{root}'"),
                        ));
                        return RuninatorType::Any;
                    }
                }
                RuninatorType::Map(values) => ty = *values,
                other => {
                    diagnostics.push(Diagnostic::error(
                        span,
                        format!("cannot access field '{key}' on {}", other.describe()),
                    ));
                    return RuninatorType::Any;
                }
            },
            PathSeg::Index(_) => match ty {
                RuninatorType::Array(element) => ty = *element,
                other => {
                    diagnostics.push(Diagnostic::error(
                        span,
                        format!("cannot index {}", other.describe()),
                    ));
                    return RuninatorType::Any;
                }
            },
        }
    }
    ty
}

fn require_orderable(ty: &RuninatorType, span: Span, diagnostics: &mut Vec<Diagnostic>) {
    if let RuninatorType::Range { base, .. } = ty {
        return require_orderable(base, span, diagnostics);
    }
    let orderable = matches!(
        ty,
        RuninatorType::Integer
            | RuninatorType::Number
            | RuninatorType::Duration
            | RuninatorType::String
            | RuninatorType::Any
            | RuninatorType::Union(_)
    );
    if !orderable {
        diagnostics.push(Diagnostic::error(
            span,
            format!("cannot order operand of type {}", ty.describe()),
        ));
    }
}

fn require_numeric(ty: &RuninatorType, span: Span, diagnostics: &mut Vec<Diagnostic>) {
    if let RuninatorType::Range { base, .. } = ty {
        return require_numeric(base, span, diagnostics);
    }
    if !matches!(
        ty,
        RuninatorType::Integer
            | RuninatorType::Number
            | RuninatorType::Duration
            | RuninatorType::Any
            | RuninatorType::Union(_)
    ) {
        diagnostics.push(Diagnostic::error(
            span,
            format!("arithmetic operand must be numeric, got {}", ty.describe()),
        ));
    }
}

fn require_stringish(ty: &RuninatorType, span: Span, diagnostics: &mut Vec<Diagnostic>) {
    let stringish = matches!(
        ty,
        RuninatorType::String | RuninatorType::Any | RuninatorType::Union(_)
    );
    if !stringish {
        diagnostics.push(Diagnostic::error(
            span,
            format!(
                "starts_with/ends_with expects strings, got {}",
                ty.describe()
            ),
        ));
    }
}

fn is_composite(ty: &RuninatorType) -> bool {
    matches!(
        ty,
        RuninatorType::Array(_) | RuninatorType::Map(_) | RuninatorType::Struct { .. }
    )
}
