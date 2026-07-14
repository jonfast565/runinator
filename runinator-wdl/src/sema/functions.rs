// validation of top-level `fn` definitions: duplicate/shadowing names, body name resolution, and the
// recursion rule (any function that can call itself, directly or mutually, must carry an
// `@recursive(max_depth: N)` annotation). function bodies may be effectful; the
// effectful-call-in-pure-position rule is enforced where the call sits (declarative positions reject
// effectful user functions), and the `goto` restriction is enforced in body resolution.

use std::collections::{BTreeSet, HashMap};

use crate::ast::{ComputeLine, Cond, CondKind, Expr, ExprKind, FnBody, FunctionDef, StrPart};
use crate::errors::WdlError;
use crate::registry::{FunctionRegistry, duplicate_errors};

use super::Diagnostic;

pub(super) fn analyze(functions: &[FunctionDef], diagnostics: &mut Vec<Diagnostic>) {
    for err in duplicate_errors(functions) {
        if let WdlError::Semantic { span, message } = err {
            diagnostics.push(Diagnostic::error(span, message));
        }
    }
    let registry = FunctionRegistry::build(functions);
    // resolve body references against each function's parameters (also enforces the goto rule).
    super::scope::resolve_function_bodies(functions, diagnostics);
    check_recursion(functions, &registry, diagnostics);
}

/// flag any function that can reach itself in the user-function call graph but lacks `@recursive`.
fn check_recursion(
    functions: &[FunctionDef],
    registry: &FunctionRegistry,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // adjacency: each function -> the user functions its body calls.
    let mut edges: HashMap<&str, BTreeSet<String>> = HashMap::new();
    for def in functions {
        let mut calls = BTreeSet::new();
        collect_user_calls_body(&def.body, registry, &mut calls);
        edges.insert(def.name.as_str(), calls);
    }
    for def in functions {
        if def.recursive.is_some() {
            continue;
        }
        if reaches_self(&def.name, &edges) {
            diagnostics.push(Diagnostic::error(
                def.span,
                format!(
                    "function '{}' is recursive and must be annotated @recursive(max_depth: N)",
                    def.name
                ),
            ));
        }
    }
}

/// whether `start` can reach itself by following call edges.
fn reaches_self(start: &str, edges: &HashMap<&str, BTreeSet<String>>) -> bool {
    let mut stack: Vec<&str> = edges
        .get(start)
        .into_iter()
        .flat_map(|set| set.iter().map(String::as_str))
        .collect();
    let mut seen = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == start {
            return true;
        }
        if !seen.insert(node.to_string()) {
            continue;
        }
        if let Some(next) = edges.get(node) {
            stack.extend(next.iter().map(String::as_str));
        }
    }
    false
}

/// collect the names of user functions called anywhere in a function body (expression or block).
fn collect_user_calls_body(body: &FnBody, registry: &FunctionRegistry, out: &mut BTreeSet<String>) {
    match body {
        FnBody::Expr(expr) => collect_user_calls(expr, registry, out),
        FnBody::Block(lines) => collect_user_calls_lines(lines, registry, out),
    }
}

/// collect user-function calls across a block's compute lines, including nested `if` conditions and
/// branches.
fn collect_user_calls_lines(
    lines: &[ComputeLine],
    registry: &FunctionRegistry,
    out: &mut BTreeSet<String>,
) {
    for line in lines {
        match line {
            ComputeLine::Let { value, .. }
            | ComputeLine::Return(value)
            | ComputeLine::Expr(value) => collect_user_calls(value, registry, out),
            ComputeLine::If {
                cond,
                then_branch,
                else_branch,
            } => {
                collect_user_calls_cond(cond, registry, out);
                collect_user_calls_lines(then_branch, registry, out);
                collect_user_calls_lines(else_branch, registry, out);
            }
            ComputeLine::Goto(_) => {}
        }
    }
}

/// collect user-function calls across a compute-tier condition.
fn collect_user_calls_cond(cond: &Cond, registry: &FunctionRegistry, out: &mut BTreeSet<String>) {
    match &cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            for part in parts {
                collect_user_calls_cond(part, registry, out);
            }
        }
        CondKind::Not(inner) => collect_user_calls_cond(inner, registry, out),
        CondKind::Expr(expr) | CondKind::Exists(expr) => collect_user_calls(expr, registry, out),
        CondKind::Cmp { left, right, .. } => {
            collect_user_calls(left, registry, out);
            collect_user_calls(right, registry, out);
        }
    }
}

/// collect the names of user functions called anywhere in `expr`.
fn collect_user_calls(expr: &Expr, registry: &FunctionRegistry, out: &mut BTreeSet<String>) {
    match &expr.kind {
        ExprKind::Call {
            name, args, named, ..
        } => {
            if registry.is_user(name) {
                out.insert(name.clone());
            }
            for arg in args.iter().chain(named.iter().map(|(_, value)| value)) {
                collect_user_calls(arg, registry, out);
            }
        }
        ExprKind::Compare { left, right, .. } => {
            collect_user_calls(left, registry, out);
            collect_user_calls(right, registry, out);
        }
        ExprKind::Ternary { cond, then, els } => {
            collect_user_calls(cond, registry, out);
            collect_user_calls(then, registry, out);
            collect_user_calls(els, registry, out);
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts)
        | ExprKind::Concat(parts)
        | ExprKind::Coalesce(parts)
        | ExprKind::Array(parts) => {
            for part in parts {
                collect_user_calls(part, registry, out);
            }
        }
        ExprKind::Neg(inner) | ExprKind::ToString(inner) | ExprKind::ToJson(inner) => {
            collect_user_calls(inner, registry, out)
        }
        ExprKind::Lambda { body, .. } => collect_user_calls(body, registry, out),
        ExprKind::Cast { expr, .. } => collect_user_calls(expr, registry, out),
        ExprKind::Apply { callee, args } => {
            collect_user_calls(callee, registry, out);
            for arg in args {
                collect_user_calls(arg, registry, out);
            }
        }
        ExprKind::Object(entries) => {
            for (_, value) in entries {
                collect_user_calls(value, registry, out);
            }
        }
        ExprKind::Str(parts) => {
            for part in parts {
                if let StrPart::Expr(inner) = part {
                    collect_user_calls(inner, registry, out);
                }
            }
        }
        ExprKind::Null
        | ExprKind::Bool(_)
        | ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::FileInclude { .. }
        | ExprKind::DirInclude { .. }
        | ExprKind::InlineCode { .. }
        | ExprKind::Path(_)
        | ExprKind::Spread(_) => {}
    }
}
