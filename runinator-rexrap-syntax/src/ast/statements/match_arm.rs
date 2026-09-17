#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    /// `Some(expr)` means an equality case; `None` (with `cond`) means a `when` case.
    pub equals: Option<Expr>,
    pub when: Option<Cond>,
    /// percentage-mode weight for this arm (the `N` in `N% -> …`).
    pub weight: Option<i64>,
    /// toggle-mode branch: `Some(true)` is the `on` arm, `Some(false)` the `off` arm.
    pub toggle: Option<bool>,
    pub body: Block,
}
