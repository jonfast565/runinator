// shared purity classification for compute blocks: a block is effectful (lowers to `std.exec` and
// dispatches to the worker) when it calls any non-pure intrinsic or reads a `secret.*` reference
// (secrets resolve late at the worker, so they can never be read in the in-process reducer). this
// single source is consulted by both lowering and sema so their views cannot drift.

use crate::ast::{ComputeLine, Cond, CondKind, Expr, ExprKind, PathSeg};
use crate::registry::FunctionRegistry;

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

/// whether an expression is effectful: it calls a non-pure intrinsic or reads a secret. a
/// user-function call is pure in itself (bodies are validated pure), so only its arguments matter.
pub(crate) fn expr_is_effectful(expr: &Expr, registry: &FunctionRegistry) -> bool {
    match &expr.kind {
        ExprKind::Call {
            name, args, named, ..
        } => {
            // higher-order intrinsics and user functions are structurally pure: only the arguments
            // can carry effects. any other unknown/effectful name forces the block to the worker.
            let structurally_pure = registry.is_user(name)
                || runinator_workflows::PureIntrinsics::contains(name)
                || runinator_workflows::is_higher_order(name);
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
