#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct IngressRouteDecl {
    pub event_type: String,
    pub lifecycle: String,
    pub action: String,
    pub intent: Option<String>,
    pub predicates: Vec<IngressPredicateDecl>,
    pub span: Span,
}
