#[allow(unused_imports)]
use super::*;

/// one parameter of a callable: its name, whether it may be omitted, and (for user functions) its
/// default expression.
#[derive(Clone)]
pub(crate) struct ParamSig {
    pub name: String,
    pub optional: bool,
    pub default: Option<Expr>,
}
