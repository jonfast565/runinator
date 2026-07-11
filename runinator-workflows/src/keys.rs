// ref and compute-statement wire keys are single-sourced in `runinator_models::workflow_ast` (where
// the structural parse/serialize lives); re-exported here for the evaluation and compute code. the
// expression/lambda keys are no longer referenced in this crate now that parse/serialize moved out.
pub(crate) use runinator_models::workflow_ast::{
    REF_CONFIG, REF_INPUT, REF_LOCAL, REF_OUTPUT, REF_PREV, REF_WORKFLOW, STMT_GOTO, STMT_LET,
    STMT_RETURN, STMT_VALUE,
};

pub(crate) const COND_ALL: &str = "all";
pub(crate) const COND_ANY: &str = "any";
pub(crate) const COND_NOT: &str = "not";
pub(crate) const COND_VALUE: &str = "value";
pub(crate) const COND_LEFT: &str = "left";
pub(crate) const COND_EQUALS: &str = "equals";
pub(crate) const COND_NOT_EQUALS: &str = "not_equals";
pub(crate) const COND_CONTAINS: &str = "contains";
pub(crate) const COND_IN: &str = "in";
pub(crate) const COND_STARTS_WITH: &str = "starts_with";
pub(crate) const COND_ENDS_WITH: &str = "ends_with";
pub(crate) const COND_GREATER_THAN: &str = "greater_than";
pub(crate) const COND_GREATER_THAN_OR_EQUAL: &str = "greater_than_or_equal";
pub(crate) const COND_LESS_THAN: &str = "less_than";
pub(crate) const COND_LESS_THAN_OR_EQUAL: &str = "less_than_or_equal";
pub(crate) const COND_EXISTS: &str = "exists";

// compute `if` shares `$if`/`then`/`else` with the expression form; kept under STMT_* names for the
// compute parser's readability.
pub(crate) const STMT_IF: &str = "$if";
pub(crate) const STMT_THEN: &str = "then";
pub(crate) const STMT_ELSE: &str = "else";

// the run-context step-outputs root; only used by ref resolution, not the wire ast.
pub(crate) const REF_STEPS: &str = "steps";
