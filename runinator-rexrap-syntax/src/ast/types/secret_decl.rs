#[allow(unused_imports)]
use super::*;

/// a single `.rexraps` declaration: `secret|config <scope>.<name…> = <literal>`. the value must be a
/// pure literal; lowering rejects references and interpolation.
#[derive(Debug, Clone, PartialEq)]
pub struct SecretDecl {
    pub is_config: bool,
    pub path: Vec<PathSeg>,
    pub value: Expr,
    pub schema: Option<Expr>,
    pub expires_at: Option<String>,
    pub span: Span,
}
