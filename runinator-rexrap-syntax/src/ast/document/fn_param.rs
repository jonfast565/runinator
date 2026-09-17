#[allow(unused_imports)]
use super::*;

/// a function parameter: a typed name, optionally marked `?` or given a `= default` (both make it
/// omittable at the call site).
#[derive(Debug, Clone, PartialEq)]
pub struct FnParam {
    pub name: String,
    pub ty: TypeExpr,
    pub optional: bool,
    pub default: Option<Expr>,
}
