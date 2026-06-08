// shared purity classification for compute blocks: a block is effectful (lowers to `std.exec` and
// dispatches to the worker) when it calls any non-pure intrinsic or reads a `secret.*` reference
// (secrets resolve late at the worker, so they can never be read in the in-process reducer). this
// single source is consulted by both lowering and sema so their views cannot drift.

use crate::ast::{ComputeLine, Cond, CondKind, Expr, ExprKind, PathSeg};

/// whether a compute block must run on the worker (`std.exec`).
pub(crate) fn block_is_effectful(body: &[ComputeLine]) -> bool {
    body.iter().any(|line| match line {
        ComputeLine::Let { value, .. } | ComputeLine::Return(value) | ComputeLine::Expr(value) => {
            expr_is_effectful(value)
        }
        ComputeLine::If {
            cond,
            then_branch,
            else_branch,
        } => {
            cond_is_effectful(cond)
                || block_is_effectful(then_branch)
                || block_is_effectful(else_branch)
        }
        ComputeLine::Goto(_) => false,
    })
}

/// whether a condition reads an effectful expression (a call or secret in an operand).
pub(crate) fn cond_is_effectful(cond: &Cond) -> bool {
    match &cond.kind {
        CondKind::All(parts) | CondKind::Any(parts) => parts.iter().any(cond_is_effectful),
        CondKind::Not(inner) => cond_is_effectful(inner),
        CondKind::Cmp { left, right, .. } => expr_is_effectful(left) || expr_is_effectful(right),
        CondKind::Exists(expr) => expr_is_effectful(expr),
    }
}

/// whether an expression is effectful: it calls a non-pure intrinsic or reads a secret.
pub(crate) fn expr_is_effectful(expr: &Expr) -> bool {
    match &expr.kind {
        ExprKind::Call { name, args } => {
            !runinator_workflows::PureIntrinsics::contains(name)
                || args.iter().any(expr_is_effectful)
        }
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
        | ExprKind::Array(parts) => parts.iter().any(expr_is_effectful),
        ExprKind::Neg(inner) | ExprKind::ToString(inner) | ExprKind::ToJson(inner) => {
            expr_is_effectful(inner)
        }
        ExprKind::Object(entries) => entries.iter().any(|(_, value)| expr_is_effectful(value)),
        _ => false,
    }
}
