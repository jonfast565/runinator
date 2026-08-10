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

use crate::types::{NamedTypes, lower_type_with, resolve_named_types};
use crate::{TypePolicy, WorkflowSignature};
use runinator_wdl_syntax::ast::*;
use runinator_wdl_syntax::errors::Span;

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
    env.check_block(&workflow.body, diagnostics);
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

impl Env {
    fn check_block(&mut self, block: &Block, diagnostics: &mut Vec<Diagnostic>) {
        // a block starts with no known predecessor; `prev` becomes concrete only after a
        // straight-line producing node (action/subflow/typed label) runs earlier in the same block.
        self.prev = RuninatorType::Any;
        for stmt in block {
            self.check_stmt(stmt, diagnostics);
            self.prev = self.predecessor_output(stmt);
        }
    }

    /// the best-effort output type a straight-line successor would see as `prev`. reuses the
    /// precomputed `node_outputs` map, so only producing nodes (action/subflow/typed label) yield a
    /// concrete type; control-flow and effect-free nodes fall back to `Any`.
    fn predecessor_output(&self, stmt: &Stmt) -> RuninatorType {
        super::effective_id(stmt)
            .and_then(|id| self.node_outputs.get(id).cloned())
            .unwrap_or(RuninatorType::Any)
    }

    fn check_stmt(&mut self, stmt: &Stmt, diagnostics: &mut Vec<Diagnostic>) {
        self.check_label_type(stmt, diagnostics);
        match &stmt.kind {
            StmtKind::Action(action) => {
                self.check_action(action, stmt.span, diagnostics);
                for (_, value) in &action.args {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Compute(compute) => {
                let base = self.scope.len();
                self.check_compute_block(&compute.body, diagnostics);
                self.scope.truncate(base);
            }
            StmtKind::Subflow(subflow) => {
                self.check_subflow(subflow, stmt.span, diagnostics);
                if let Some(run_name) = &subflow.run_name {
                    self.check_expr(run_name, diagnostics);
                }
                for (_, value) in &subflow.params {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Wait(_) => {}
            StmtKind::Output(output) => {
                if let Some(data) = &output.data {
                    self.check_expr(data, diagnostics);
                }
                for (_, source) in &output.items {
                    self.check_expr(source, diagnostics);
                }
            }
            StmtKind::Yield(value) => self.check_expr(value, diagnostics),
            StmtKind::Input(input) => {
                if let Some(prompt) = &input.prompt {
                    self.check_expr(prompt, diagnostics);
                }
            }
            StmtKind::Approval(approval) => {
                self.check_expr(&approval.prompt, diagnostics);
                for (_, value) in &approval.metadata {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Gate(gate) => {
                if let Some(when) = &gate.when {
                    self.check_cond(when, diagnostics);
                }
                for (_, value) in &gate.metadata {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Signal(signal) => {
                for (_, value) in &signal.metadata {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Config(config) => {
                if let Some(name) = &config.name {
                    self.check_expr(name, diagnostics);
                }
                if let Some(metadata) = &config.metadata {
                    self.check_expr(metadata, diagnostics);
                }
            }
            StmtKind::Fail(message) => {
                if let Some(message) = message {
                    self.check_expr(message, diagnostics);
                }
            }
            StmtKind::If(if_stmt) => {
                for (cond, body) in &if_stmt.arms {
                    self.check_cond(cond, diagnostics);
                    self.check_block(body, diagnostics);
                }
                if let Some(else_block) = &if_stmt.else_block {
                    self.check_block(else_block, diagnostics);
                }
            }
            StmtKind::For(for_stmt) => {
                let element = self.check_iterable(&for_stmt.items, "for loop", diagnostics);
                self.scope.push((for_stmt.var.clone(), element));
                self.check_block(&for_stmt.body, diagnostics);
                self.scope.pop();
            }
            StmtKind::While(while_stmt) => {
                self.check_cond(&while_stmt.cond, diagnostics);
                self.check_block(&while_stmt.body, diagnostics);
            }
            StmtKind::Map(map_stmt) => {
                let element = self.check_iterable(&map_stmt.items, "map", diagnostics);
                self.scope.push((map_stmt.var.clone(), element));
                self.check_block(&map_stmt.body, diagnostics);
                self.scope.pop();
            }
            StmtKind::Match(match_stmt) => {
                self.check_expr(&match_stmt.subject, diagnostics);
                for arm in &match_stmt.arms {
                    if let Some(equals) = &arm.equals {
                        self.check_expr(equals, diagnostics);
                    }
                    if let Some(when) = &arm.when {
                        self.check_cond(when, diagnostics);
                    }
                    self.check_block(&arm.body, diagnostics);
                }
                if let Some(default) = &match_stmt.default {
                    self.check_block(default, diagnostics);
                }
            }
            StmtKind::Parallel(parallel) => {
                for branch in &parallel.branches {
                    self.check_block(branch, diagnostics);
                }
            }
            StmtKind::Race(race) => {
                for branch in &race.branches {
                    self.check_block(branch, diagnostics);
                }
            }
            // `resume` carries no expressions, names, or bindings, so every pass is a no-op.
            StmtKind::Resume(_) => {}
            StmtKind::Try(try_stmt) => {
                self.check_block(&try_stmt.body, diagnostics);
                if let Some(catch) = &try_stmt.catch {
                    self.check_block(catch, diagnostics);
                }
                if let Some(finally) = &try_stmt.finally {
                    self.check_block(finally, diagnostics);
                }
            }
            StmtKind::Assert(assert) => {
                for (_, cond) in &assert.assertions {
                    self.check_cond(cond, diagnostics);
                }
            }
            StmtKind::Transform(transform) => {
                for (_, value) in &transform.bindings {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Audit(audit) => {
                self.check_expr(&audit.action, diagnostics);
                for value in [
                    audit.actor.as_ref(),
                    audit.target.as_ref(),
                    audit.reason.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    self.check_expr(value, diagnostics);
                }
            }
            StmtKind::Await(await_stmt) => {
                if let Some(key) = &await_stmt.key {
                    self.check_expr(key, diagnostics);
                }
            }
            StmtKind::Debounce(debounce) => {
                if let Some(key) = &debounce.key {
                    self.check_expr(key, diagnostics);
                }
            }
            StmtKind::EventSource(es) => {
                if let Some(filter) = &es.filter {
                    self.check_cond(filter, diagnostics);
                }
            }
            StmtKind::Mutex(mutex) => self.check_block(&mutex.body, diagnostics),
            // these carry no expressions to type-check.
            StmtKind::Checkpoint(_)
            | StmtKind::Throttle(_)
            | StmtKind::Cooldown(_)
            | StmtKind::Collect(_)
            | StmtKind::Barrier(_)
            | StmtKind::CircuitBreaker(_) => {}
        }
    }

    fn check_label_type(&self, stmt: &Stmt, diagnostics: &mut Vec<Diagnostic>) {
        let Some(label_type) = &stmt.label_type else {
            return;
        };
        let declared = lower_type_with(label_type, &self.named).unwrap_or(RuninatorType::Any);
        let inferred = match &stmt.kind {
            StmtKind::Action(action) => self
                .provider_actions
                .get(&(action.provider.clone(), action.function.clone()))
                .map(ActionMetadata::results_type),
            StmtKind::Subflow(subflow) => Some(subflow_output_type(
                subflow,
                self.workflow_signatures.get(&subflow.workflow_name),
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

    fn check_action(&self, action: &ActionStmt, span: Span, diagnostics: &mut Vec<Diagnostic>) {
        let key = (action.provider.clone(), action.function.clone());
        let Some(metadata) = self.provider_actions.get(&key) else {
            if self.provider_catalog_present && self.type_policy == TypePolicy::Strict {
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
            let actual = self.infer_expr(value, diagnostics);
            check_assignable(
                &actual,
                &param.ty,
                &format!("action parameter '{}'", param.name),
                value.span,
                diagnostics,
            );
        }
    }

    fn check_subflow(&self, subflow: &SubflowStmt, span: Span, diagnostics: &mut Vec<Diagnostic>) {
        let Some(signature) = self.workflow_signatures.get(&subflow.workflow_name) else {
            if self.type_policy == TypePolicy::Strict {
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
                .map(|(name, value)| (name.clone(), self.infer_expr(value, diagnostics))),
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
        &self,
        items: &Expr,
        label: &str,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> RuninatorType {
        let ty = self.infer_expr(items, diagnostics);
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

    fn check_cond(&self, cond: &Cond, diagnostics: &mut Vec<Diagnostic>) {
        match &cond.kind {
            CondKind::All(parts) | CondKind::Any(parts) => {
                for part in parts {
                    self.check_cond(part, diagnostics);
                }
            }
            CondKind::Not(inner) => self.check_cond(inner, diagnostics),
            CondKind::Expr(expr) => {
                let ty = self.infer_expr(expr, diagnostics);
                check_assignable(
                    &ty,
                    &RuninatorType::Boolean,
                    "condition",
                    expr.span,
                    diagnostics,
                );
            }
            CondKind::Exists(expr) => self.check_expr(expr, diagnostics),
            CondKind::Cmp { left, op, right } => {
                let left_ty = self.infer_expr(left, diagnostics);
                let right_ty = self.infer_expr(right, diagnostics);
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

    fn check_expr(&self, expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
        match &expr.kind {
            ExprKind::ToString(inner) => {
                let ty = self.infer_expr(inner, diagnostics);
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
                        self.check_expr(inner, diagnostics);
                    }
                }
            }
            ExprKind::Array(items) => {
                for item in items {
                    self.check_expr(item, diagnostics);
                }
            }
            ExprKind::Object(entries) => {
                for (_, value) in entries {
                    self.check_expr(value, diagnostics);
                }
            }
            ExprKind::Concat(parts) | ExprKind::Coalesce(parts) => {
                for part in parts {
                    self.check_expr(part, diagnostics);
                }
            }
            ExprKind::ToJson(inner) => {
                let ty = self.infer_expr(inner, diagnostics);
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
                let ty = self.infer_expr(inner, diagnostics);
                require_numeric(&ty, inner.span, diagnostics);
            }
            ExprKind::Add(parts)
            | ExprKind::Sub(parts)
            | ExprKind::Mul(parts)
            | ExprKind::Div(parts)
            | ExprKind::Mod(parts) => {
                for part in parts {
                    let ty = self.infer_expr(part, diagnostics);
                    require_numeric(&ty, part.span, diagnostics);
                }
            }
            ExprKind::Compare { left, right, .. } => {
                self.check_expr(left, diagnostics);
                self.check_expr(right, diagnostics);
            }
            ExprKind::Ternary { cond, then, els } => {
                let cond_ty = self.infer_expr(cond, diagnostics);
                check_assignable(
                    &cond_ty,
                    &RuninatorType::Boolean,
                    "ternary condition",
                    cond.span,
                    diagnostics,
                );
                // disjoint branches are not an error: they unify to a union (see `infer_expr`).
                self.check_expr(then, diagnostics);
                self.check_expr(els, diagnostics);
            }
            ExprKind::Call {
                name, args, named, ..
            } => {
                if runinator_compute::is_higher_order(name) {
                    let _ = self.infer_higher_order_call_type(name, args, expr.span, diagnostics);
                    return;
                }
                // a call to a local bound to a first-class lambda: check argument count (its
                // parameter types are unconstrained `any`, so only arity is enforced).
                if let Some(RuninatorType::Function { params, .. }) = self.function_local(name) {
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
                        self.check_expr(arg, diagnostics);
                    }
                    return;
                }
                // check each positional argument against the intrinsic's declared parameter type,
                // skipping opaque (`any`) types on either side to avoid false positives on refs.
                if let Some(sig) = runinator_compute::intrinsic_signature(name) {
                    for (param, arg) in sig.parameters.iter().zip(args.iter()) {
                        let arg_ty = self.infer_expr(arg, diagnostics);
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
                    self.check_expr(arg, diagnostics);
                }
            }
            // a lambda body is checked permissively; its params type as `Any` (unknown reference
            // heads stay opaque), so no spurious diagnostics arise from the bound names.
            ExprKind::Lambda { body, .. } => self.check_expr(body, diagnostics),
            // a cast asserts the inner value has the target type: check the inner expression, then
            // that its inferred type is assignable to the target (opaque `any` values pass, which is
            // the point — `parse_json(s) as T` and `[] as T[]` adopt `T` here).
            ExprKind::Cast { expr: inner, ty } => {
                self.check_expr(inner, diagnostics);
                let declared = lower_type_with(ty, &self.named).unwrap_or(RuninatorType::Any);
                let actual = self.infer_expr(inner, diagnostics);
                check_assignable(&actual, &declared, "cast", expr.span, diagnostics);
            }
            // applying a callee value: check the callee and arguments, then that the callee is a
            // function of matching arity. an opaque (`any`) callee stays permissive.
            ExprKind::Apply { callee, args } => {
                self.check_expr(callee, diagnostics);
                for arg in args {
                    self.check_expr(arg, diagnostics);
                }
                match self.infer_expr(callee, diagnostics) {
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
                let _ = self.infer_expr(expr, diagnostics);
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

    /// type-check a compute block: thread typed locals through `let` (so later lines see them),
    /// check each `let x: T` value against its annotation, and recurse into nested `if` branches
    /// with block scoping.
    fn check_compute_block(
        &mut self,
        body: &[runinator_wdl_syntax::ast::ComputeLine],
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        use runinator_wdl_syntax::ast::ComputeLine;
        for line in body {
            match line {
                ComputeLine::Let { name, ty, value } => {
                    self.check_expr(value, diagnostics);
                    let declared = ty
                        .as_ref()
                        .map(|t| lower_type_with(t, &self.named).unwrap_or(RuninatorType::Any));
                    // for `let f: function<(A) -> R> = <lambda>`, bind the lambda's parameters to
                    // the declared parameter types so the body checks against the annotation
                    // (bidirectional), rather than typing every parameter `Any`.
                    let value_ty = match (&declared, &value.kind) {
                        (
                            Some(RuninatorType::Function {
                                params: expected, ..
                            }),
                            ExprKind::Lambda { params, body },
                        ) if params.len() == expected.len() => {
                            let mut scoped = self.clone();
                            for (param, param_ty) in params.iter().zip(expected.iter()) {
                                scoped.scope.push((param.clone(), param_ty.clone()));
                            }
                            let ret = scoped.infer_expr(body, diagnostics);
                            RuninatorType::Function {
                                params: expected.clone(),
                                ret: Box::new(ret),
                            }
                        }
                        _ => self.infer_expr(value, diagnostics),
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
                    self.scope.push((name.clone(), local_ty));
                }
                ComputeLine::Return(value) | ComputeLine::Expr(value) => {
                    self.check_expr(value, diagnostics)
                }
                ComputeLine::If {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    let base = self.scope.len();
                    self.check_compute_block(then_branch, diagnostics);
                    self.scope.truncate(base);
                    self.check_compute_block(else_branch, diagnostics);
                    self.scope.truncate(base);
                }
                ComputeLine::Goto(_) => {}
            }
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

impl Env {
    fn infer_expr(&self, expr: &Expr, diagnostics: &mut Vec<Diagnostic>) -> RuninatorType {
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
                    let ty = self.infer_expr(part, diagnostics);
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
                    let item_ty = self.infer_expr(item, diagnostics);
                    match &element {
                        None => element = Some(item_ty),
                        Some(existing) => {
                            if let Some(common) = common_type(existing, &item_ty) {
                                element = Some(common);
                            } else {
                                if self.type_policy == TypePolicy::Strict {
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
                    .map(|(key, value)| (key.clone(), self.infer_expr(value, diagnostics))),
            ),
            ExprKind::Path(segs) => self.infer_path(segs, expr.span, diagnostics),
            // arithmetic yields a number; intrinsic call results are author-time opaque.
            ExprKind::Add(parts)
            | ExprKind::Sub(parts)
            | ExprKind::Mul(parts)
            | ExprKind::Div(parts)
            | ExprKind::Mod(parts) => self.numeric_result_type(parts, diagnostics),
            ExprKind::Neg(inner) => {
                let ty = self.infer_expr(inner, diagnostics);
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
                let then_ty = self.infer_expr(then, diagnostics);
                let els_ty = self.infer_expr(els, diagnostics);
                then_ty.unify(&els_ty)
            }
            // a call's result type comes from the intrinsic signature or, for higher-order
            // intrinsics, from the collection and lambda argument types.
            ExprKind::Call { name, args, .. } => {
                if runinator_compute::is_higher_order(name) {
                    self.infer_higher_order_call_type(name, args, expr.span, diagnostics)
                } else if let Some(RuninatorType::Function { ret, .. }) = self.function_local(name)
                {
                    // a call to a local bound to a first-class lambda yields the function's result
                    // type.
                    (**ret).clone()
                } else {
                    // infer argument types into a throwaway sink (the check pass already validates
                    // the arguments) so the polymorphic intrinsics recover an argument-dependent
                    // result type before falling back to the catalog's declared (often `any`) result.
                    let mut sink = Vec::new();
                    let arg_types = args
                        .iter()
                        .map(|arg| self.infer_expr(arg, &mut sink))
                        .collect::<Vec<_>>();
                    // extract any literal key(s) from the second argument for key-driven intrinsics.
                    let literal_keys = args
                        .get(1)
                        .and_then(runinator_wdl_syntax::ast::static_string_keys);
                    runinator_compute::intrinsic_result_type(
                        name,
                        &arg_types,
                        literal_keys.as_deref(),
                    )
                    .or_else(|| {
                        runinator_compute::intrinsic_signature(name)
                            .and_then(|sig| sig.results.first().map(|result| result.ty.clone()))
                    })
                    .unwrap_or(RuninatorType::Any)
                }
            }
            // a lambda's value type: its parameters are unconstrained in a value position (`Any`)
            // and its result is the body's type with the parameters in scope.
            ExprKind::Lambda { params, body } => {
                let mut scoped = self.clone();
                for param in params {
                    scoped.scope.push((param.clone(), RuninatorType::Any));
                }
                let ret = scoped.infer_expr(body, diagnostics);
                RuninatorType::Function {
                    params: params.iter().map(|_| RuninatorType::Any).collect(),
                    ret: Box::new(ret),
                }
            }
            // a cast's inferred type is exactly the asserted target: this is what lets an opaque
            // inner value (`parse_json`, `[]`) resolve to a concrete shape at the cast position.
            ExprKind::Cast { ty, .. } => {
                lower_type_with(ty, &self.named).unwrap_or(RuninatorType::Any)
            }
            // applying a callee value yields the callee function's result type, or `Any` when the
            // callee is opaque or not a function (the check pass reports a genuine non-function
            // application).
            ExprKind::Apply { callee, .. } => match self.infer_expr(callee, diagnostics) {
                RuninatorType::Function { ret, .. } => *ret,
                _ => RuninatorType::Any,
            },
            // spreads are expanded before sema runs; treat as untyped if one is reached.
            ExprKind::Spread(_) => RuninatorType::Any,
        }
    }

    fn infer_higher_order_call_type(
        &self,
        name: &str,
        args: &[Expr],
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
        let collection_type = self.infer_expr(collection, diagnostics);
        let item_type = collection_item_type(name, &collection_type, collection.span, diagnostics);
        match name {
            "map" => {
                let body_type =
                    self.infer_lambda_type(name, args.get(1), &[(0, item_type)], span, diagnostics);
                RuninatorType::array(body_type)
            }
            "flat_map" => {
                let body_type =
                    self.infer_lambda_type(name, args.get(1), &[(0, item_type)], span, diagnostics);
                match body_type {
                    RuninatorType::Array(inner) => RuninatorType::array(*inner),
                    other => RuninatorType::array(other),
                }
            }
            "filter" => {
                let body_type = self.infer_lambda_type(
                    name,
                    args.get(1),
                    &[(0, item_type.clone())],
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
                let body_type = self.infer_lambda_type(
                    name,
                    args.get(1),
                    &[(0, item_type.clone())],
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
                    self.infer_lambda_type(name, args.get(1), &[(0, item_type)], span, diagnostics);
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
                let body_type = self.infer_lambda_type(
                    name,
                    args.get(1),
                    &[(0, item_type.clone())],
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
                    .map(|arg| self.infer_expr(arg, diagnostics))
                    .unwrap_or_else(|| {
                        diagnostics.push(Diagnostic::error(
                            span,
                            "'reduce' is missing an initial accumulator argument",
                        ));
                        RuninatorType::Any
                    });
                let body_type = self.infer_lambda_type(
                    name,
                    args.get(2),
                    &[(0, accumulator_type.clone()), (1, item_type)],
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

impl Env {
    fn numeric_result_type(
        &self,
        parts: &[Expr],
        diagnostics: &mut Vec<Diagnostic>,
    ) -> RuninatorType {
        let mut all_integer = true;
        for part in parts {
            let ty = self.infer_expr(part, diagnostics);
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
    fn function_local(&self, name: &str) -> Option<&RuninatorType> {
        self.scope
            .iter()
            .rev()
            .find(|(local, ty)| local == name && matches!(ty, RuninatorType::Function { .. }))
            .map(|(_, ty)| ty)
    }

    fn infer_lambda_type(
        &self,
        name: &str,
        expr: Option<&Expr>,
        bindings: &[(usize, RuninatorType)],
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
            // a first-class function value passed by reference: use its declared result type when
            // the arity matches; stay permissive for an opaque (`any`) reference.
            let ty = self.infer_expr(expr, diagnostics);
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
        let mut scoped = self.clone();
        for (index, ty) in bindings {
            scoped.scope.push((params[*index].clone(), ty.clone()));
        }
        scoped.check_expr(body, diagnostics);
        scoped.infer_expr(body, diagnostics)
    }

    fn infer_path(
        &self,
        segs: &[PathSeg],
        span: Span,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> RuninatorType {
        let Some(PathSeg::Key(head)) = segs.first() else {
            return RuninatorType::Any;
        };
        let rest = &segs[1..];
        // a loop/map variable shadows everything else; params and typed node outputs follow.
        if let Some((_, ty)) = self.scope.iter().rev().find(|(name, _)| name == head) {
            return navigate(ty.clone(), rest, head, span, diagnostics);
        }
        if head == "params" {
            return navigate(self.input.clone(), rest, head, span, diagnostics);
        }
        if let Some(ty) = self.node_outputs.get(head) {
            return navigate(ty.clone(), rest, head, span, diagnostics);
        }
        // `prev` resolves to the source-order predecessor's output type when it is a producing
        // node; it is `Any` at ambiguous positions (first node, after control flow, nested blocks).
        if head == "prev" {
            return navigate(self.prev.clone(), rest, head, span, diagnostics);
        }
        // `run` exposes the current run's metadata; its shape is single-sourced from the runtime
        // header.
        if head == "run" {
            let run_type =
                runinator_models::workflow_state::WorkflowContextHeader::runinator_type();
            return navigate(run_type, rest, head, span, diagnostics);
        }
        // a bare node reference with no recorded output shape is opaque author-time.
        RuninatorType::Any
    }
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
