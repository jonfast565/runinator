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

pub(crate) const EXPR_VALUE: &str = "$value";
pub(crate) const EXPR_REF: &str = "$ref";
pub(crate) const EXPR_CONCAT: &str = "$concat";
pub(crate) const EXPR_COALESCE: &str = "$coalesce";
pub(crate) const EXPR_LITERAL: &str = "$literal";
pub(crate) const EXPR_TO_STRING: &str = "$to_string";
pub(crate) const EXPR_TO_JSON_STRING: &str = "$to_json_string";
pub(crate) const EXPR_NODE: &str = "$node";

// arithmetic operators (array form, mirroring $concat).
pub(crate) const EXPR_ADD: &str = "$add";
pub(crate) const EXPR_SUB: &str = "$sub";
pub(crate) const EXPR_MUL: &str = "$mul";
pub(crate) const EXPR_DIV: &str = "$div";
pub(crate) const EXPR_MOD: &str = "$mod";
pub(crate) const EXPR_NEG: &str = "$neg";

// intrinsic library call: { "$call": "add", "args": [<expr>...] }.
pub(crate) const EXPR_CALL: &str = "$call";
pub(crate) const EXPR_ARGS: &str = "args";

// a lambda passed to a higher-order intrinsic: { "$lambda": { "params": ["x"], "body": <expr> } }.
pub(crate) const EXPR_LAMBDA: &str = "$lambda";
pub(crate) const LAMBDA_PARAMS: &str = "params";
pub(crate) const LAMBDA_BODY: &str = "body";

// a lazy conditional expression: { "$if": <cond>, "then": <expr>, "else": <expr> }. only the taken
// branch is evaluated, so a recursive function's base case can terminate.
pub(crate) const EXPR_IF: &str = "$if";
pub(crate) const EXPR_THEN: &str = "then";
pub(crate) const EXPR_ELSE: &str = "else";

// compute program statements.
pub(crate) const STMT_LET: &str = "$let";
pub(crate) const STMT_RETURN: &str = "$return";
pub(crate) const STMT_GOTO: &str = "$goto";
pub(crate) const STMT_IF: &str = "$if";
pub(crate) const STMT_VALUE: &str = "value";
pub(crate) const STMT_THEN: &str = "then";
pub(crate) const STMT_ELSE: &str = "else";

pub(crate) const REF_NODE: &str = "node";
pub(crate) const REF_OUTPUT: &str = "output";
pub(crate) const REF_INPUT: &str = "input";
pub(crate) const REF_PREV: &str = "prev";
pub(crate) const REF_WORKFLOW: &str = "workflow";
pub(crate) const REF_CONFIG: &str = "config";
pub(crate) const REF_STEPS: &str = "steps";
pub(crate) const REF_LOCAL: &str = "let";
