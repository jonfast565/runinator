#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct IngressDecl {
    pub scope: String,
    pub routes: Vec<IngressRouteDecl>,
    pub span: Span,
}
