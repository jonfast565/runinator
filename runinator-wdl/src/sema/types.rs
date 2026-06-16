// type checking. seeds an environment from the workflow parameter type and infers expression
// types from there, reusing the `RuninatorType` algebra in runinator-models. only facts the
// front end can know author-time are enforced: parameter field access, iterable `for`/`map`
// sources, orderable comparison operands, and `string()`/`json()` argument kinds. action and
// subflow results, `prev`, and `run` are `Any`, so references through them stay permissive.

use runinator_models::types::RuninatorType;

use crate::ast::*;
use crate::errors::Span;
use crate::lower::types::{NamedTypes, lower_type_with, resolve_named_types};

use super::Diagnostic;

/// the typing environment: the workflow parameter type, declared named types, and active loop/map
/// and compute-local variable types.
struct Env {
    input: RuninatorType,
    named: NamedTypes,
    scope: Vec<(String, RuninatorType)>,
}

pub(super) fn analyze(workflow: &Workflow, diagnostics: &mut Vec<Diagnostic>) {
    // resolve declared type names (ignoring cycle/duplicate errors, which scope/lowering report) so
    // parameter and annotation types referencing them type-check against the resolved shape.
    let named = resolve_named_types(&workflow.type_decls).unwrap_or_default();
    let input = workflow
        .input
        .as_ref()
        .and_then(|type_expr| lower_type_with(type_expr, &named).ok())
        .unwrap_or(RuninatorType::Any);
    let mut env = Env {
        input,
        named,
        scope: Vec::new(),
    };
    check_block(&workflow.body, &mut env, diagnostics);
}

fn check_block(block: &Block, env: &mut Env, diagnostics: &mut Vec<Diagnostic>) {
    for stmt in block {
        check_stmt(stmt, env, diagnostics);
    }
}

fn check_stmt(stmt: &Stmt, env: &mut Env, diagnostics: &mut Vec<Diagnostic>) {
    match &stmt.kind {
        StmtKind::Action(action) => {
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
        }
        StmtKind::Deliverable(deliverable) => {
            for (_, source) in &deliverable.items {
                check_expr(source, env, diagnostics);
            }
        }
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
    }
}

/// require an iterable source and return its element type (`Any` when unknown).
fn check_iterable(
    items: &Expr,
    env: &Env,
    label: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> RuninatorType {
    let ty = infer_expr(items, env, diagnostics);
    match ty {
        RuninatorType::Array(element) => *element,
        RuninatorType::Any | RuninatorType::Union(_) => RuninatorType::Any,
        other => {
            diagnostics.push(Diagnostic::error(
                items.span,
                format!("{label} expects an array, got {}", other.describe()),
            ));
            RuninatorType::Any
        }
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
        CondKind::Expr(expr) => check_expr(expr, env, diagnostics),
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
        ExprKind::ToJson(inner) | ExprKind::Neg(inner) => check_expr(inner, env, diagnostics),
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts) => {
            for part in parts {
                check_expr(part, env, diagnostics);
            }
        }
        ExprKind::Compare { left, right, .. } => {
            check_expr(left, env, diagnostics);
            check_expr(right, env, diagnostics);
        }
        ExprKind::Ternary { cond, then, els } => {
            check_expr(cond, env, diagnostics);
            check_expr(then, env, diagnostics);
            check_expr(els, env, diagnostics);
        }
        ExprKind::Call {
            name, args, named, ..
        } => {
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
                let value_ty = infer_expr(value, env, diagnostics);
                let declared = ty
                    .as_ref()
                    .map(|t| lower_type_with(t, &env.named).unwrap_or(RuninatorType::Any));
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
    if actual.validate_assignable_to(expected).is_err() {
        diagnostics.push(Diagnostic::error(
            span,
            format!(
                "{label} expects {}, got {}",
                expected.describe(),
                actual.describe()
            ),
        ));
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
        ExprKind::Coalesce(_) => RuninatorType::Any,
        ExprKind::Array(items) => {
            let mut element: Option<RuninatorType> = None;
            for item in items {
                let item_ty = infer_expr(item, env, diagnostics);
                match &element {
                    None => element = Some(item_ty),
                    Some(existing) if *existing == item_ty => {}
                    Some(_) => return RuninatorType::array(RuninatorType::Any),
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
        ExprKind::Add(_)
        | ExprKind::Sub(_)
        | ExprKind::Mul(_)
        | ExprKind::Div(_)
        | ExprKind::Mod(_)
        | ExprKind::Neg(_) => RuninatorType::Number,
        // a relational comparison resolves to a boolean.
        ExprKind::Compare { .. } => RuninatorType::Boolean,
        // a ternary resolves to its branches' common type, or `any` when they differ.
        ExprKind::Ternary { then, els, .. } => {
            let then_ty = infer_expr(then, env, diagnostics);
            let els_ty = infer_expr(els, env, diagnostics);
            if then_ty == els_ty {
                then_ty
            } else {
                RuninatorType::Any
            }
        }
        // a call's result type comes from the intrinsic signature (its first declared result).
        ExprKind::Call { name, .. } => runinator_workflows::intrinsic_signature(name)
            .and_then(|sig| sig.results.first().map(|result| result.ty.clone()))
            .unwrap_or(RuninatorType::Any),
        // a lambda carries no value type of its own.
        ExprKind::Lambda { .. } => RuninatorType::Any,
        // spreads are expanded before sema runs; treat as untyped if one is reached.
        ExprKind::Spread(_) => RuninatorType::Any,
    }
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
    // a loop/map variable shadows everything else; params is the only other typed root.
    if let Some((_, ty)) = env.scope.iter().rev().find(|(name, _)| name == head) {
        return navigate(ty.clone(), rest, head, span, diagnostics);
    }
    if head == "params" {
        return navigate(env.input.clone(), rest, head, span, diagnostics);
    }
    // prev/run/node references are opaque author-time.
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
    for seg in segs {
        if matches!(ty, RuninatorType::Any | RuninatorType::Union(_)) {
            return RuninatorType::Any;
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
    let orderable = matches!(
        ty,
        RuninatorType::Integer
            | RuninatorType::Number
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
