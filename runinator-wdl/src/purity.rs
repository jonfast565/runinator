// shared purity classification for compute blocks: a block is effectful (lowers to `std.exec` and
// dispatches to the worker) when it calls any non-pure intrinsic or reads a `secret.*` reference
// (secrets resolve late at the worker, so they can never be read in the in-process reducer). this
// single source is consulted by both lowering and sema so their views cannot drift.

use crate::ast::{ComputeLine, Cond, CondKind, Expr, ExprKind, FnBody, PathSeg};
use crate::registry::FunctionRegistry;

/// whether a function body is effectful: an expression body reuses the expression rule, a block body
/// the compute-block rule. used both to route calls onto the worker and to compute per-function
/// effectfulness in the registry.
pub(crate) fn fn_body_is_effectful(body: &FnBody, registry: &FunctionRegistry) -> bool {
    match body {
        FnBody::Expr(expr) => expr_is_effectful(expr, registry),
        FnBody::Block(lines) => block_is_effectful(lines, registry),
    }
}

/// whether a compute block must run on the worker (`std.exec`).
pub(crate) fn block_is_effectful(body: &[ComputeLine], registry: &FunctionRegistry) -> bool {
    body.iter().any(|line| match line {
        ComputeLine::Let { value, .. } | ComputeLine::Return(value) | ComputeLine::Expr(value) => {
            expr_is_effectful(value, registry)
        }
        ComputeLine::If {
            cond,
            then_branch,
            else_branch,
        } => {
            cond_is_effectful(cond, registry)
                || block_is_effectful(then_branch, registry)
                || block_is_effectful(else_branch, registry)
        }
        ComputeLine::Goto(_) => false,
    })
}

/// whether a condition reads an effectful expression (a call or secret in an operand).
pub(crate) fn cond_is_effectful(cond: &Cond, registry: &FunctionRegistry) -> bool {
    match &cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => {
            parts.iter().any(|part| cond_is_effectful(part, registry))
        }
        CondKind::Not(inner) => cond_is_effectful(inner, registry),
        CondKind::Expr(expr) => expr_is_effectful(expr, registry),
        CondKind::Cmp { left, right, .. } => {
            expr_is_effectful(left, registry) || expr_is_effectful(right, registry)
        }
        CondKind::Exists(expr) => expr_is_effectful(expr, registry),
    }
}

/// whether an expression is effectful: it calls a non-pure intrinsic, calls an effectful user
/// function, or reads a secret. a pure user-function call is pure in itself, so only its arguments
/// matter.
pub(crate) fn expr_is_effectful(expr: &Expr, registry: &FunctionRegistry) -> bool {
    match &expr.kind {
        ExprKind::Call {
            name, args, named, ..
        } => {
            // a structurally pure callable (a pure intrinsic, a higher-order intrinsic, or a pure
            // user function) carries no effect itself, so only its arguments can. an effectful
            // callable, or any unknown name, forces the enclosing block to the worker. the registry
            // is the single source for this, seeded from the intrinsic metadata `pure` bit.
            let structurally_pure = registry.knows(name) && !registry.is_effectful(name);
            !structurally_pure
                || args.iter().any(|arg| expr_is_effectful(arg, registry))
                || named
                    .iter()
                    .any(|(_, value)| expr_is_effectful(value, registry))
        }
        ExprKind::Lambda { body, .. } => expr_is_effectful(body, registry),
        // a secret reference forces the block to the worker, where secrets resolve.
        ExprKind::Path(segs) => {
            matches!(segs.first(), Some(PathSeg::Key(head)) if head == "secret")
        }
        ExprKind::Add(parts)
        | ExprKind::Sub(parts)
        | ExprKind::Mul(parts)
        | ExprKind::Div(parts)
        | ExprKind::Mod(parts)
        | ExprKind::Concat(parts)
        | ExprKind::Coalesce(parts)
        | ExprKind::Array(parts) => parts.iter().any(|part| expr_is_effectful(part, registry)),
        ExprKind::Neg(inner) | ExprKind::ToString(inner) | ExprKind::ToJson(inner) => {
            expr_is_effectful(inner, registry)
        }
        // the comparison intrinsic is pure, but either operand may itself be effectful.
        ExprKind::Compare { left, right, .. } => {
            expr_is_effectful(left, registry) || expr_is_effectful(right, registry)
        }
        // a ternary is effectful if its condition or either branch is.
        ExprKind::Ternary { cond, then, els } => {
            expr_is_effectful(cond, registry)
                || expr_is_effectful(then, registry)
                || expr_is_effectful(els, registry)
        }
        ExprKind::Object(entries) => entries
            .iter()
            .any(|(_, value)| expr_is_effectful(value, registry)),
        _ => false,
    }
}
