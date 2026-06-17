use base64::Engine as _;
use runinator_models::providers::{ActionMetadata, ParameterMetadata, ResultMetadata};
use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};

use crate::conditions::evaluate_condition_env;
use crate::errors::WorkflowValidationError;
use crate::expressions::{evaluate_expression_with, parse_expression};
use crate::functions::{EvalEnv, FunctionTable};
use crate::keys::{
    REF_LOCAL, STMT_ELSE, STMT_GOTO, STMT_IF, STMT_LET, STMT_RETURN, STMT_THEN, STMT_VALUE,
};
use crate::types::WorkflowExpression;

/// a namespaced, typed library of functions callable from a compute program. the reducer installs
/// only the pure subset; the worker installs a superset that adds effectful ops.
pub trait IntrinsicLibrary {
    /// invoke `name` with already-evaluated `args`.
    fn call(&self, name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError>;
    /// whether the library exposes `name`.
    fn knows(&self, name: &str) -> bool;
    /// whether `name` is pure (reducer-evaluable).
    fn is_pure(&self, name: &str) -> bool;
}

/// a single statement in a compute program.
#[derive(Debug, Clone, PartialEq)]
pub enum ComputeStmt {
    Let {
        name: String,
        value: WorkflowExpression,
    },
    Return(WorkflowExpression),
    Goto(String),
    If {
        condition: Value,
        then_branch: ComputeProgram,
        else_branch: ComputeProgram,
    },
    Expr(WorkflowExpression),
}

/// an ordered list of compute statements.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ComputeProgram(pub Vec<ComputeStmt>);

/// the result of running a compute program.
#[derive(Debug, Clone, PartialEq)]
pub enum ComputeOutcome {
    /// an explicit `return <value>`.
    Return(Value),
    /// an explicit `goto <target>` (pure programs only).
    Goto(String),
    /// the program ended without returning or jumping.
    Fallthrough(Value),
}

/// parse a lowered compute program (a JSON array of statements).
pub fn parse_program(value: &Value) -> Result<ComputeProgram, WorkflowValidationError> {
    let items = value.as_array().ok_or_else(|| {
        WorkflowValidationError::InvalidComputeProgram("program must be an array".into())
    })?;
    let statements = items
        .iter()
        .map(parse_statement)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(ComputeProgram(statements))
}

fn parse_statement(value: &Value) -> Result<ComputeStmt, WorkflowValidationError> {
    let object = value.as_object().ok_or_else(|| {
        WorkflowValidationError::InvalidComputeProgram("statement must be an object".into())
    })?;
    if let Some(name) = object.get(STMT_LET) {
        let name = name.as_str().ok_or_else(|| {
            WorkflowValidationError::InvalidComputeProgram("$let name must be a string".into())
        })?;
        let bound = object.get(STMT_VALUE).ok_or_else(|| {
            WorkflowValidationError::InvalidComputeProgram("$let requires a value".into())
        })?;
        return Ok(ComputeStmt::Let {
            name: name.to_string(),
            value: parse_expression(bound)?,
        });
    }
    if let Some(bound) = object.get(STMT_RETURN) {
        return Ok(ComputeStmt::Return(parse_expression(bound)?));
    }
    if let Some(target) = object.get(STMT_GOTO) {
        let target = target.as_str().ok_or_else(|| {
            WorkflowValidationError::InvalidComputeProgram("$goto target must be a string".into())
        })?;
        return Ok(ComputeStmt::Goto(target.to_string()));
    }
    if let Some(condition) = object.get(STMT_IF) {
        let then_branch = match object.get(STMT_THEN) {
            Some(branch) => parse_program(branch)?,
            None => ComputeProgram::default(),
        };
        let else_branch = match object.get(STMT_ELSE) {
            Some(branch) => parse_program(branch)?,
            None => ComputeProgram::default(),
        };
        return Ok(ComputeStmt::If {
            condition: condition.clone(),
            then_branch,
            else_branch,
        });
    }
    // anything else is a bare expression statement (e.g. a side-effecting call).
    Ok(ComputeStmt::Expr(parse_expression(value)?))
}

/// run a compute program against `context` using `lib` to resolve calls. `let` bindings layer
/// locals into the `let` slot of a working copy of the context; `return`/`goto` short-circuit.
pub fn run_program(
    program: &ComputeProgram,
    context: &Value,
    lib: &dyn IntrinsicLibrary,
) -> Result<ComputeOutcome, WorkflowValidationError> {
    run_program_with(program, context, lib, None)
}

/// like `run_program`, but also resolving calls to the workflow's user-defined functions through
/// `functions`. the reducer/worker pass the definition's function table here.
pub fn run_program_with(
    program: &ComputeProgram,
    context: &Value,
    lib: &dyn IntrinsicLibrary,
    functions: Option<&FunctionTable>,
) -> Result<ComputeOutcome, WorkflowValidationError> {
    let mut working = context.clone();
    ensure_local_slot(&mut working);
    let env = EvalEnv::new(Some(lib), functions);
    match run_block(&program.0, &mut working, env)? {
        Some(outcome) => Ok(outcome),
        None => Ok(ComputeOutcome::Fallthrough(Value::Null)),
    }
}

/// run a sequence of compute statements against `context` at the given `env`. `let` bindings layer
/// into the `let` slot; `return`/`goto` short-circuit with `Some(outcome)`, fallthrough yields
/// `None`. user-function bodies reuse this so a block body evaluates exactly like a compute block.
pub(crate) fn run_block(
    statements: &[ComputeStmt],
    context: &mut Value,
    env: EvalEnv,
) -> Result<Option<ComputeOutcome>, WorkflowValidationError> {
    for statement in statements {
        match statement {
            ComputeStmt::Let { name, value } => {
                let evaluated = evaluate_expression_with(value, context, env)?;
                set_local(context, name, evaluated);
            }
            ComputeStmt::Return(expr) => {
                let value = evaluate_expression_with(expr, context, env)?;
                return Ok(Some(ComputeOutcome::Return(value)));
            }
            ComputeStmt::Goto(target) => {
                return Ok(Some(ComputeOutcome::Goto(target.clone())));
            }
            ComputeStmt::Expr(expr) => {
                evaluate_expression_with(expr, context, env)?;
            }
            ComputeStmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let branch = if evaluate_condition_env(condition, context, env)? {
                    &then_branch.0
                } else {
                    &else_branch.0
                };
                if let Some(outcome) = run_block(branch, context, env)? {
                    return Ok(Some(outcome));
                }
            }
        }
    }
    Ok(None)
}

fn ensure_local_slot(context: &mut Value) {
    if let Some(object) = context.as_object_mut()
        && !object.get(REF_LOCAL).is_some_and(Value::is_object)
    {
        object.insert(REF_LOCAL.into(), Value::Object(Map::new()));
    }
}

fn set_local(context: &mut Value, name: &str, value: Value) {
    if let Some(locals) = context.get_mut(REF_LOCAL).and_then(Value::as_object_mut) {
        locals.insert(name.to_string(), value);
    }
}

/// the pure standard-library intrinsics, shared by the reducer, sema, and the `std` provider so
/// their views of which functions exist and are pure cannot drift.
pub struct PureIntrinsics;

impl PureIntrinsics {
    /// the names of every pure intrinsic, in stable order.
    pub fn names() -> &'static [&'static str] {
        &[
            // numeric.
            "add",
            "sub",
            "mul",
            "div",
            "mod",
            "floor",
            "ceil",
            "round",
            "min",
            "max",
            "parse_int",
            "parse_float",
            // strings.
            "lower",
            "upper",
            "trim",
            "split",
            "join",
            "replace",
            "substring",
            "starts_with",
            "ends_with",
            // collections.
            "len",
            "keys",
            "values",
            "contains",
            "at",
            "has",
            "sum",
            "sort",
            "reverse",
            "unique",
            "flatten",
            "slice",
            "first",
            "last",
            "append",
            "range",
            // objects.
            "merge",
            "pick",
            "omit",
            "entries",
            "from_entries",
            // encoding.
            "parse_json",
            "base64_encode",
            "base64_decode",
            // logic / comparison.
            "eq",
            "ne",
            "gt",
            "lt",
            "gte",
            "lte",
            "not",
            "and",
            "or",
            "default",
            // dates.
            "format_date",
            "parse_date",
            "add_duration",
            "date_diff",
            // regex.
            "regex_match",
            "regex_replace",
            "regex_extract",
        ]
    }

    /// whether `name` is a pure intrinsic.
    pub fn contains(name: &str) -> bool {
        Self::names().contains(&name)
    }

    /// typed signatures for the pure intrinsics, used to build provider metadata.
    pub fn signatures() -> Vec<ActionMetadata> {
        let numeric = |name: &str| {
            ActionMetadata::new(name, format!("pure numeric intrinsic {name}"))
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Number),
                    ParameterMetadata::required("b", RuninatorType::Number),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Number)])
                .pure()
        };
        let unary_number_to_int = |name: &str| {
            ActionMetadata::new(name, format!("pure intrinsic {name}"))
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::Number,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
                .pure()
        };
        let unary_string = |name: &str| {
            ActionMetadata::new(name, format!("pure string intrinsic {name}"))
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure()
        };
        // two opaque operands yielding a boolean (comparison/membership predicates).
        let any_predicate = |name: &str| {
            ActionMetadata::new(name, format!("pure predicate intrinsic {name}"))
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("b", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Boolean)])
                .pure()
        };
        // a whole-array transform yielding an array.
        let array_transform = |name: &str| {
            ActionMetadata::new(name, format!("pure collection intrinsic {name}"))
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure()
        };
        vec![
            numeric("add"),
            numeric("sub"),
            numeric("mul"),
            numeric("div"),
            numeric("mod"),
            ActionMetadata::new("len", "length of a string, array, or object")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
                .pure(),
            ActionMetadata::new("keys", "keys of an object")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::String),
                )])
                .pure(),
            unary_string("lower"),
            unary_string("upper"),
            unary_number_to_int("floor"),
            unary_number_to_int("ceil"),
            unary_number_to_int("round"),
            numeric("min"),
            numeric("max"),
            ActionMetadata::new("parse_int", "parse an integer from a string")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
                .pure(),
            ActionMetadata::new("parse_float", "parse a number from a string")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Number)])
                .pure(),
            // strings.
            unary_string("trim"),
            ActionMetadata::new("split", "split a string into parts on a separator")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("sep", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::String),
                )])
                .pure(),
            ActionMetadata::new("join", "join an array into a string with a separator")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("sep", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new("replace", "replace all occurrences of a substring")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("from", RuninatorType::String),
                    ParameterMetadata::required("to", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new("substring", "slice a string by character index")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("start", RuninatorType::Integer),
                    ParameterMetadata::optional("end", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            any_predicate("starts_with"),
            any_predicate("ends_with"),
            // collections.
            ActionMetadata::new("values", "values of an object")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            any_predicate("contains"),
            any_predicate("has"),
            ActionMetadata::new("at", "element at an array index or object key")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("key", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("sum", "sum of a numeric array")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Number)])
                .pure(),
            array_transform("sort"),
            array_transform("reverse"),
            array_transform("unique"),
            array_transform("flatten"),
            ActionMetadata::new("slice", "slice an array by index range")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("start", RuninatorType::Integer),
                    ParameterMetadata::optional("end", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            ActionMetadata::new("first", "first element of an array")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("last", "last element of an array")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("append", "append an element to an array")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("item", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            ActionMetadata::new("range", "an integer range [start, end)")
                .with_parameters(vec![
                    ParameterMetadata::required("start", RuninatorType::Integer),
                    ParameterMetadata::required("end", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Integer),
                )])
                .pure(),
            // objects.
            ActionMetadata::new("merge", "shallow-merge two objects (right wins)")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("b", RuninatorType::Any),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("pick", "keep only the named keys of an object")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required(
                        "keys",
                        RuninatorType::array(RuninatorType::String),
                    ),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("omit", "drop the named keys of an object")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required(
                        "keys",
                        RuninatorType::array(RuninatorType::String),
                    ),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            ActionMetadata::new("entries", "an object as an array of {key, value} pairs")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::Any),
                )])
                .pure(),
            ActionMetadata::new("from_entries", "build an object from {key, value} pairs")
                .with_parameters(vec![ParameterMetadata::required("a", RuninatorType::Any)])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            // encoding.
            ActionMetadata::new("parse_json", "parse a JSON string into a value")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::String,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
                .pure(),
            unary_string("base64_encode"),
            unary_string("base64_decode"),
            // logic / comparison.
            any_predicate("eq"),
            any_predicate("ne"),
            any_predicate("gt"),
            any_predicate("lt"),
            any_predicate("gte"),
            any_predicate("lte"),
            ActionMetadata::new("not", "logical negation of a boolean")
                .with_parameters(vec![ParameterMetadata::required(
                    "a",
                    RuninatorType::Boolean,
                )])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::Boolean)])
                .pure(),
            any_predicate("and"),
            any_predicate("or"),
            ActionMetadata::new(
                "default",
                "the first argument when non-null, else the second",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::Any),
                ParameterMetadata::required("b", RuninatorType::Any),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
            .pure(),
            // dates.
            ActionMetadata::new("format_date", "format a timestamp with a strftime pattern")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::Any),
                    ParameterMetadata::required("fmt", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new(
                "parse_date",
                "parse a timestamp into RFC 3339 with a pattern",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::String),
                ParameterMetadata::required("fmt", RuninatorType::String),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
            .pure(),
            ActionMetadata::new("add_duration", "add seconds to an RFC 3339 timestamp")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("seconds", RuninatorType::Integer),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new(
                "date_diff",
                "seconds between two RFC 3339 timestamps (a - b)",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::String),
                ParameterMetadata::required("b", RuninatorType::String),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Integer)])
            .pure(),
            // regex.
            ActionMetadata::new(
                "regex_match",
                "whether a pattern matches anywhere in a string",
            )
            .with_parameters(vec![
                ParameterMetadata::required("a", RuninatorType::String),
                ParameterMetadata::required("pattern", RuninatorType::String),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Boolean)])
            .pure(),
            ActionMetadata::new("regex_replace", "replace all pattern matches in a string")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("pattern", RuninatorType::String),
                    ParameterMetadata::required("replacement", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new("result", RuninatorType::String)])
                .pure(),
            ActionMetadata::new("regex_extract", "all full matches of a pattern in a string")
                .with_parameters(vec![
                    ParameterMetadata::required("a", RuninatorType::String),
                    ParameterMetadata::required("pattern", RuninatorType::String),
                ])
                .with_results(vec![ResultMetadata::new(
                    "result",
                    RuninatorType::array(RuninatorType::String),
                )])
                .pure(),
        ]
    }
}

impl IntrinsicLibrary for PureIntrinsics {
    fn call(&self, name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError> {
        call_pure(name, args)
    }

    fn knows(&self, name: &str) -> bool {
        Self::contains(name)
    }

    fn is_pure(&self, _name: &str) -> bool {
        true
    }
}

/// the names of the effectful (`std.exec`-only) intrinsics. this is the shared vocabulary the std
/// provider implements and sema validates against, so neither can drift.
pub const EFFECTFUL_INTRINSIC_NAMES: &[&str] = &["http_get", "http_post", "now", "uuid", "env"];

/// typed signatures for the effectful intrinsics. the std provider builds its metadata from these
/// (so the implementation and the advertised contract stay in sync), and sema type-checks calls
/// against them.
pub fn effectful_signatures() -> Vec<ActionMetadata> {
    let response = || {
        RuninatorType::structure([
            ("status", RuninatorType::Integer),
            ("body", RuninatorType::Any),
        ])
    };
    vec![
        ActionMetadata::new("http_get", "perform an HTTP GET request")
            .with_parameters(vec![ParameterMetadata::required(
                "url",
                RuninatorType::String,
            )])
            .with_results(vec![ResultMetadata::new("response", response())]),
        ActionMetadata::new("http_post", "perform an HTTP POST request")
            .with_parameters(vec![
                ParameterMetadata::required("url", RuninatorType::String),
                ParameterMetadata::optional("body", RuninatorType::Any),
            ])
            .with_results(vec![ResultMetadata::new("response", response())]),
        ActionMetadata::new("now", "current UTC timestamp in RFC 3339")
            .with_results(vec![ResultMetadata::new("result", RuninatorType::String)]),
        ActionMetadata::new("uuid", "generate a random UUID v4")
            .with_results(vec![ResultMetadata::new("result", RuninatorType::String)]),
        ActionMetadata::new("env", "read an environment variable")
            .with_parameters(vec![ParameterMetadata::required(
                "name",
                RuninatorType::String,
            )])
            .with_results(vec![ResultMetadata::new(
                "result",
                RuninatorType::Union(vec![RuninatorType::String, RuninatorType::Null]),
            )]),
    ]
}

/// the typed signature of any intrinsic (pure or effectful), if known.
pub fn intrinsic_signature(name: &str) -> Option<ActionMetadata> {
    PureIntrinsics::signatures()
        .into_iter()
        .chain(effectful_signatures())
        .find(|action| action.function_name == name)
}

/// the higher-order intrinsics. these take a lambda argument and are evaluated by the expression
/// engine itself (not dispatched through `IntrinsicLibrary::call`), because applying a lambda needs
/// the evaluator and the run context, which the library does not have. they are structurally pure:
/// a call is pure when its collection and lambda body are pure.
pub const HIGHER_ORDER_NAMES: &[&str] = &[
    "map", "filter", "find", "any", "all", "reduce", "sort_by", "flat_map",
];

/// whether `name` is a higher-order intrinsic handled by the expression engine.
pub fn is_higher_order(name: &str) -> bool {
    HIGHER_ORDER_NAMES.contains(&name)
}

/// whether `name` is any known intrinsic (pure, effectful, or higher-order).
pub fn is_known_intrinsic(name: &str) -> bool {
    PureIntrinsics::contains(name)
        || EFFECTFUL_INTRINSIC_NAMES.contains(&name)
        || is_higher_order(name)
}

/// the reserved root namespace for the builtin standard library.
pub const STD_NAMESPACE: &str = "std";

/// the std module names, in stable order, used for completion and decompile grouping.
pub const STD_MODULES: &[&str] = &[
    "math",
    "strings",
    "collections",
    "objects",
    "encoding",
    "logic",
    "dates",
    "regex",
    "exec",
];

/// the std module an intrinsic leaf belongs to (the single segment between `std` and the leaf).
/// single source of truth for surface qualification, sema validation, and decompile rendering; the
/// runtime dispatch (`call_pure` and the worker std provider) still keys on the bare leaf.
pub fn intrinsic_module(leaf: &str) -> Option<&'static str> {
    let module = match leaf {
        "add" | "sub" | "mul" | "div" | "mod" | "floor" | "ceil" | "round" | "min" | "max"
        | "parse_int" | "parse_float" => "math",
        "lower" | "upper" | "trim" | "split" | "join" | "replace" | "substring" | "starts_with"
        | "ends_with" => "strings",
        "len" | "keys" | "values" | "contains" | "at" | "has" | "sum" | "sort" | "reverse"
        | "unique" | "flatten" | "slice" | "first" | "last" | "append" | "range" | "map"
        | "filter" | "find" | "any" | "all" | "reduce" | "sort_by" | "flat_map" => "collections",
        "merge" | "pick" | "omit" | "entries" | "from_entries" => "objects",
        "parse_json" | "base64_encode" | "base64_decode" => "encoding",
        "eq" | "ne" | "gt" | "lt" | "gte" | "lte" | "not" | "and" | "or" | "default" => "logic",
        "format_date" | "parse_date" | "add_duration" | "date_diff" => "dates",
        "regex_match" | "regex_replace" | "regex_extract" => "regex",
        "http_get" | "http_post" | "now" | "uuid" | "env" => "exec",
        _ => return None,
    };
    Some(module)
}

/// the fully-qualified surface name of an intrinsic leaf, e.g. `add` -> `std.math.add`.
pub fn qualified_intrinsic_name(leaf: &str) -> Option<String> {
    intrinsic_module(leaf).map(|module| format!("{STD_NAMESPACE}.{module}.{leaf}"))
}

/// resolve a fully-qualified std path (`std.<module>.<leaf>`) to its bare leaf, validating that the
/// module segment is the one the leaf actually lives in. `Err(Some(module))` names the leaf's real
/// module on a mismatch; `Err(None)` means the leaf is not a std intrinsic at all.
pub fn resolve_std_path(module: &str, leaf: &str) -> Result<&'static str, Option<&'static str>> {
    match intrinsic_module(leaf) {
        Some(actual) if actual == module => Ok(actual),
        Some(actual) => Err(Some(actual)),
        None => Err(None),
    }
}

/// the accepted argument count range `(min, max)` for an intrinsic, or `None` if unknown. used by
/// sema to flag obvious arity mistakes (a typo'd name returns `None` and is rejected separately).
pub fn intrinsic_arity(name: &str) -> Option<(usize, usize)> {
    let arity = match name {
        "add" | "sub" | "mul" | "div" | "mod" | "min" | "max" => (2, 2),
        "len" | "keys" | "lower" | "upper" | "floor" | "ceil" | "round" | "parse_int"
        | "parse_float" => (1, 1),
        // strings.
        "trim" | "base64_encode" | "base64_decode" | "parse_json" => (1, 1),
        "split" | "join" | "starts_with" | "ends_with" => (2, 2),
        "replace" => (3, 3),
        "substring" => (2, 3),
        // collections.
        "values" | "sum" | "sort" | "reverse" | "unique" | "flatten" | "first" | "last"
        | "entries" | "from_entries" => (1, 1),
        "contains" | "has" | "at" | "append" | "range" => (2, 2),
        "slice" => (2, 3),
        // objects.
        "merge" | "pick" | "omit" => (2, 2),
        // logic / comparison.
        "eq" | "ne" | "gt" | "lt" | "gte" | "lte" | "and" | "or" | "default" => (2, 2),
        "not" => (1, 1),
        // dates.
        "format_date" | "parse_date" | "add_duration" | "date_diff" => (2, 2),
        // regex.
        "regex_match" | "regex_extract" => (2, 2),
        "regex_replace" => (3, 3),
        // higher-order: a collection plus a lambda; reduce also takes an initial accumulator.
        "map" | "filter" | "find" | "any" | "all" | "sort_by" | "flat_map" => (2, 2),
        "reduce" => (3, 3),
        "http_get" | "env" => (1, 1),
        "http_post" => (1, 2),
        "now" | "uuid" => (0, 0),
        _ => return None,
    };
    Some(arity)
}

/// dispatch a pure intrinsic by name. exposed so a superset library (the worker) can delegate.
pub fn call_pure(name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError> {
    match name {
        "add" => numeric_binary(name, args, |a, b| a + b, |a, b| Some(a.wrapping_add(b))),
        "sub" => numeric_binary(name, args, |a, b| a - b, |a, b| Some(a.wrapping_sub(b))),
        "mul" => numeric_binary(name, args, |a, b| a * b, |a, b| Some(a.wrapping_mul(b))),
        "div" => numeric_binary(name, args, |a, b| a / b, i64::checked_div),
        "mod" => numeric_binary(name, args, |a, b| a % b, i64::checked_rem),
        "min" => numeric_binary(name, args, f64::min, |a, b| Some(a.min(b))),
        "max" => numeric_binary(name, args, f64::max, |a, b| Some(a.max(b))),
        "len" => intrinsic_len(args),
        "keys" => intrinsic_keys(args),
        "lower" => string_unary(name, args, |s| s.to_lowercase()),
        "upper" => string_unary(name, args, |s| s.to_uppercase()),
        "trim" => string_unary(name, args, |s| s.trim().to_string()),
        "floor" => number_to_int(name, args, f64::floor),
        "ceil" => number_to_int(name, args, f64::ceil),
        "round" => number_to_int(name, args, f64::round),
        "parse_int" => parse_int(args),
        "parse_float" => parse_float(args),
        // strings.
        "split" => intrinsic_split(args),
        "join" => intrinsic_join(args),
        "replace" => intrinsic_replace(args),
        "substring" => intrinsic_substring(args),
        "starts_with" => string_pair(name, args, |s, p| s.starts_with(p)),
        "ends_with" => string_pair(name, args, |s, p| s.ends_with(p)),
        // collections.
        "values" => intrinsic_values(args),
        "contains" => intrinsic_contains(args),
        "has" => intrinsic_has(args),
        "at" => intrinsic_at(args),
        "sum" => intrinsic_sum(args),
        "sort" => intrinsic_sort(args),
        "reverse" => intrinsic_reverse(args),
        "unique" => intrinsic_unique(args),
        "flatten" => intrinsic_flatten(args),
        "slice" => intrinsic_slice(args),
        "first" => intrinsic_first(args),
        "last" => intrinsic_last(args),
        "append" => intrinsic_append(args),
        "range" => intrinsic_range(args),
        // objects.
        "merge" => intrinsic_merge(args),
        "pick" => intrinsic_pick(args, true),
        "omit" => intrinsic_pick(args, false),
        "entries" => intrinsic_entries(args),
        "from_entries" => intrinsic_from_entries(args),
        // encoding.
        "parse_json" => intrinsic_parse_json(args),
        "base64_encode" => intrinsic_base64_encode(args),
        "base64_decode" => intrinsic_base64_decode(args),
        // logic / comparison.
        "eq" => Ok(Value::Bool(arg(name, args, 0)? == arg(name, args, 1)?)),
        "ne" => Ok(Value::Bool(arg(name, args, 0)? != arg(name, args, 1)?)),
        "gt" => compare(name, args, |o| o.is_gt()),
        "lt" => compare(name, args, |o| o.is_lt()),
        "gte" => compare(name, args, |o| o.is_ge()),
        "lte" => compare(name, args, |o| o.is_le()),
        "not" => Ok(Value::Bool(!truthy(&arg(name, args, 0)?))),
        "and" => Ok(Value::Bool(
            truthy(&arg(name, args, 0)?) && truthy(&arg(name, args, 1)?),
        )),
        "or" => Ok(Value::Bool(
            truthy(&arg(name, args, 0)?) || truthy(&arg(name, args, 1)?),
        )),
        "default" => {
            let primary = arg(name, args, 0)?;
            Ok(if primary.is_null() {
                arg(name, args, 1)?
            } else {
                primary
            })
        }
        // dates.
        "format_date" => intrinsic_format_date(args),
        "parse_date" => intrinsic_parse_date(args),
        "add_duration" => intrinsic_add_duration(args),
        "date_diff" => intrinsic_date_diff(args),
        // regex.
        "regex_match" => intrinsic_regex_match(args),
        "regex_replace" => intrinsic_regex_replace(args),
        "regex_extract" => intrinsic_regex_extract(args),
        _ => Err(WorkflowValidationError::UnknownIntrinsic(name.to_string())),
    }
}

fn arg(name: &str, args: &[Value], index: usize) -> Result<Value, WorkflowValidationError> {
    args.get(index)
        .cloned()
        .ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: name.to_string(),
            message: format!("missing argument {index}"),
        })
}

fn number(name: &str, value: &Value) -> Result<f64, WorkflowValidationError> {
    value
        .as_f64()
        .ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: name.to_string(),
            message: format!("expected a number, got {value}"),
        })
}

fn numeric_binary(
    name: &str,
    args: &[Value],
    float_op: impl Fn(f64, f64) -> f64,
    int_op: impl Fn(i64, i64) -> Option<i64>,
) -> Result<Value, WorkflowValidationError> {
    let left = arg(name, args, 0)?;
    let right = arg(name, args, 1)?;
    if let (Some(a), Some(b)) = (left.as_i64(), right.as_i64()) {
        let result = int_op(a, b).ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: name.to_string(),
            message: "undefined integer operation".into(),
        })?;
        return Ok(Value::from(result));
    }
    let result = float_op(number(name, &left)?, number(name, &right)?);
    finite_number(name, result)
}

fn string_unary(
    name: &str,
    args: &[Value],
    op: impl Fn(&str) -> String,
) -> Result<Value, WorkflowValidationError> {
    let value = arg(name, args, 0)?;
    let text = value
        .as_str()
        .ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: name.to_string(),
            message: format!("expected a string, got {value}"),
        })?;
    Ok(Value::String(op(text)))
}

fn number_to_int(
    name: &str,
    args: &[Value],
    op: impl Fn(f64) -> f64,
) -> Result<Value, WorkflowValidationError> {
    let value = arg(name, args, 0)?;
    let result = op(number(name, &value)?);
    Ok(Value::from(result as i64))
}

fn intrinsic_len(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("len", args, 0)?;
    let length = match &value {
        Value::String(text) => text.chars().count(),
        Value::Array(items) => items.len(),
        Value::Object(object) => object.len(),
        other => {
            return Err(WorkflowValidationError::IntrinsicError {
                name: "len".into(),
                message: format!("expected a string, array, or object, got {other}"),
            });
        }
    };
    Ok(Value::from(length as i64))
}

fn intrinsic_keys(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("keys", args, 0)?;
    let Some(object) = value.as_object() else {
        return Err(WorkflowValidationError::IntrinsicError {
            name: "keys".into(),
            message: format!("expected an object, got {value}"),
        });
    };
    Ok(Value::Array(
        object
            .keys()
            .map(|key| Value::String(key.clone()))
            .collect(),
    ))
}

fn parse_int(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("parse_int", args, 0)?;
    let text = value
        .as_str()
        .ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: "parse_int".into(),
            message: format!("expected a string, got {value}"),
        })?;
    text.trim().parse::<i64>().map(Value::from).map_err(|err| {
        WorkflowValidationError::IntrinsicError {
            name: "parse_int".into(),
            message: err.to_string(),
        }
    })
}

fn parse_float(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("parse_float", args, 0)?;
    let text = value
        .as_str()
        .ok_or_else(|| WorkflowValidationError::IntrinsicError {
            name: "parse_float".into(),
            message: format!("expected a string, got {value}"),
        })?;
    let parsed =
        text.trim()
            .parse::<f64>()
            .map_err(|err| WorkflowValidationError::IntrinsicError {
                name: "parse_float".into(),
                message: err.to_string(),
            })?;
    finite_number("parse_float", parsed)
}

fn finite_number(name: &str, value: f64) -> Result<Value, WorkflowValidationError> {
    if value.is_finite() {
        return Ok(Value::from(value));
    }
    Err(WorkflowValidationError::IntrinsicError {
        name: name.to_string(),
        message: "produced a non-finite number".into(),
    })
}

fn err(name: &str, message: impl Into<String>) -> WorkflowValidationError {
    WorkflowValidationError::IntrinsicError {
        name: name.to_string(),
        message: message.into(),
    }
}

fn string_at(name: &str, args: &[Value], index: usize) -> Result<String, WorkflowValidationError> {
    let value = arg(name, args, index)?;
    value.as_str().map(str::to_string).ok_or_else(|| {
        err(
            name,
            format!("expected a string at position {index}, got {value}"),
        )
    })
}

fn int_at(name: &str, args: &[Value], index: usize) -> Result<i64, WorkflowValidationError> {
    let value = arg(name, args, index)?;
    value.as_i64().ok_or_else(|| {
        err(
            name,
            format!("expected an integer at position {index}, got {value}"),
        )
    })
}

fn array_at(
    name: &str,
    args: &[Value],
    index: usize,
) -> Result<Vec<Value>, WorkflowValidationError> {
    let value = arg(name, args, index)?;
    value.as_array().map(Clone::clone).ok_or_else(|| {
        err(
            name,
            format!("expected an array at position {index}, got {value}"),
        )
    })
}

// null and boolean false are falsy; every other value is truthy.
fn truthy(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Bool(false))
}

// stringify a scalar for join/format; objects and arrays serialize to json.
fn stringify(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

// total order over scalars: numbers before strings before everything else; used by sort.
pub(crate) fn cmp_values(left: &Value, right: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if let (Some(a), Some(b)) = (left.as_f64(), right.as_f64()) {
        return a.partial_cmp(&b).unwrap_or(Ordering::Equal);
    }
    if let (Some(a), Some(b)) = (left.as_str(), right.as_str()) {
        return a.cmp(b);
    }
    Ordering::Equal
}

fn string_pair(
    name: &str,
    args: &[Value],
    predicate: impl FnOnce(&str, &str) -> bool,
) -> Result<Value, WorkflowValidationError> {
    let left = string_at(name, args, 0)?;
    let right = string_at(name, args, 1)?;
    Ok(Value::Bool(predicate(&left, &right)))
}

fn compare(
    name: &str,
    args: &[Value],
    predicate: impl FnOnce(std::cmp::Ordering) -> bool,
) -> Result<Value, WorkflowValidationError> {
    let left = arg(name, args, 0)?;
    let right = arg(name, args, 1)?;
    if let (Some(a), Some(b)) = (left.as_f64(), right.as_f64()) {
        let ordering = a
            .partial_cmp(&b)
            .ok_or_else(|| err(name, "comparison is undefined"))?;
        return Ok(Value::Bool(predicate(ordering)));
    }
    if let (Some(a), Some(b)) = (left.as_str(), right.as_str()) {
        return Ok(Value::Bool(predicate(a.cmp(b))));
    }
    Err(err(name, "comparison requires two numbers or two strings"))
}

// clamp a possibly-negative index against `len`, where a negative index counts from the end.
fn clamp_index(index: i64, len: usize) -> usize {
    if index < 0 {
        return (len as i64 + index).max(0) as usize;
    }
    (index as usize).min(len)
}

fn intrinsic_split(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("split", args, 0)?;
    let sep = string_at("split", args, 1)?;
    let parts = if sep.is_empty() {
        text.chars().map(|c| Value::String(c.to_string())).collect()
    } else {
        text.split(&sep)
            .map(|part| Value::String(part.to_string()))
            .collect()
    };
    Ok(Value::Array(parts))
}

fn intrinsic_join(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let items = array_at("join", args, 0)?;
    let sep = string_at("join", args, 1)?;
    let parts = items.iter().map(stringify).collect::<Vec<_>>();
    Ok(Value::String(parts.join(&sep)))
}

fn intrinsic_replace(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("replace", args, 0)?;
    let from = string_at("replace", args, 1)?;
    let to = string_at("replace", args, 2)?;
    Ok(Value::String(text.replace(&from, &to)))
}

fn intrinsic_substring(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let chars = string_at("substring", args, 0)?.chars().collect::<Vec<_>>();
    let start = clamp_index(int_at("substring", args, 1)?, chars.len());
    let end = match args.get(2) {
        Some(_) => clamp_index(int_at("substring", args, 2)?, chars.len()),
        None => chars.len(),
    };
    let slice = if start <= end {
        chars[start..end].iter().collect::<String>()
    } else {
        String::new()
    };
    Ok(Value::String(slice))
}

fn intrinsic_values(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("values", args, 0)?;
    let object = value
        .as_object()
        .ok_or_else(|| err("values", format!("expected an object, got {value}")))?;
    Ok(Value::Array(object.values().cloned().collect()))
}

fn intrinsic_contains(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let haystack = arg("contains", args, 0)?;
    let needle = arg("contains", args, 1)?;
    if let (Some(text), Some(sub)) = (haystack.as_str(), needle.as_str()) {
        return Ok(Value::Bool(text.contains(sub)));
    }
    if let Some(items) = haystack.as_array() {
        return Ok(Value::Bool(items.iter().any(|item| item == &needle)));
    }
    if let (Some(object), Some(key)) = (haystack.as_object(), needle.as_str()) {
        return Ok(Value::Bool(object.contains_key(key)));
    }
    Err(err("contains", "requires a string, array, or object"))
}

fn intrinsic_has(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let collection = arg("has", args, 0)?;
    let key = arg("has", args, 1)?;
    if let (Some(object), Some(key)) = (collection.as_object(), key.as_str()) {
        return Ok(Value::Bool(object.contains_key(key)));
    }
    if let (Some(items), Some(index)) = (collection.as_array(), key.as_i64()) {
        return Ok(Value::Bool(index >= 0 && (index as usize) < items.len()));
    }
    Ok(Value::Bool(false))
}

fn intrinsic_at(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let collection = arg("at", args, 0)?;
    let key = arg("at", args, 1)?;
    if let (Some(items), Some(index)) = (collection.as_array(), key.as_i64()) {
        let resolved = clamp_index(index, items.len());
        return Ok(items.get(resolved).cloned().unwrap_or(Value::Null));
    }
    if let (Some(object), Some(key)) = (collection.as_object(), key.as_str()) {
        return Ok(object.get(key).cloned().unwrap_or(Value::Null));
    }
    Ok(Value::Null)
}

fn intrinsic_sum(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let items = array_at("sum", args, 0)?;
    let mut int_acc: i64 = 0;
    let mut float_acc: f64 = 0.0;
    let mut all_int = true;
    for item in &items {
        if all_int && let Some(value) = item.as_i64() {
            int_acc = int_acc.wrapping_add(value);
            float_acc += value as f64;
            continue;
        }
        let value = item
            .as_f64()
            .ok_or_else(|| err("sum", format!("expected a number, got {item}")))?;
        all_int = false;
        float_acc += value;
    }
    if all_int {
        Ok(Value::from(int_acc))
    } else {
        finite_number("sum", float_acc)
    }
}

fn intrinsic_sort(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let mut items = array_at("sort", args, 0)?;
    items.sort_by(cmp_values);
    Ok(Value::Array(items))
}

fn intrinsic_reverse(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("reverse", args, 0)?;
    if let Some(text) = value.as_str() {
        return Ok(Value::String(text.chars().rev().collect()));
    }
    let mut items = array_at("reverse", args, 0)?;
    items.reverse();
    Ok(Value::Array(items))
}

fn intrinsic_unique(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let items = array_at("unique", args, 0)?;
    let mut seen: Vec<Value> = Vec::new();
    for item in items {
        if !seen.contains(&item) {
            seen.push(item);
        }
    }
    Ok(Value::Array(seen))
}

fn intrinsic_flatten(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let items = array_at("flatten", args, 0)?;
    let mut out = Vec::new();
    for item in items {
        match item {
            Value::Array(inner) => out.extend(inner),
            other => out.push(other),
        }
    }
    Ok(Value::Array(out))
}

fn intrinsic_slice(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let items = array_at("slice", args, 0)?;
    let start = clamp_index(int_at("slice", args, 1)?, items.len());
    let end = match args.get(2) {
        Some(_) => clamp_index(int_at("slice", args, 2)?, items.len()),
        None => items.len(),
    };
    let slice = if start <= end {
        items[start..end].to_vec()
    } else {
        Vec::new()
    };
    Ok(Value::Array(slice))
}

fn intrinsic_first(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    Ok(array_at("first", args, 0)?
        .first()
        .cloned()
        .unwrap_or(Value::Null))
}

fn intrinsic_last(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    Ok(array_at("last", args, 0)?
        .last()
        .cloned()
        .unwrap_or(Value::Null))
}

fn intrinsic_append(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let mut items = array_at("append", args, 0)?;
    items.push(arg("append", args, 1)?);
    Ok(Value::Array(items))
}

fn intrinsic_range(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let start = int_at("range", args, 0)?;
    let end = int_at("range", args, 1)?;
    Ok(Value::Array((start..end).map(Value::from).collect()))
}

fn intrinsic_merge(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let left = arg("merge", args, 0)?;
    let right = arg("merge", args, 1)?;
    let (Some(left), Some(right)) = (left.as_object(), right.as_object()) else {
        return Err(err("merge", "both arguments must be objects"));
    };
    let mut merged = left.clone();
    for (key, value) in right {
        merged.insert(key.clone(), value.clone());
    }
    Ok(Value::Object(merged))
}

// keep (or drop) the named keys of an object; `keep` selects pick vs omit.
fn intrinsic_pick(args: &[Value], keep: bool) -> Result<Value, WorkflowValidationError> {
    let name = if keep { "pick" } else { "omit" };
    let value = arg(name, args, 0)?;
    let object = value
        .as_object()
        .ok_or_else(|| err(name, "first argument must be an object"))?;
    let keys = array_at(name, args, 1)?;
    let wanted = keys
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut out = Map::new();
    for (key, value) in object {
        if wanted.iter().any(|k| k == key) == keep {
            out.insert(key.clone(), value.clone());
        }
    }
    Ok(Value::Object(out))
}

fn intrinsic_entries(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let value = arg("entries", args, 0)?;
    let object = value
        .as_object()
        .ok_or_else(|| err("entries", format!("expected an object, got {value}")))?;
    let entries = object
        .iter()
        .map(|(key, value)| {
            Value::Object(Map::from_iter([
                ("key".into(), Value::String(key.clone())),
                ("value".into(), value.clone()),
            ]))
        })
        .collect();
    Ok(Value::Array(entries))
}

fn intrinsic_from_entries(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let items = array_at("from_entries", args, 0)?;
    let mut out = Map::new();
    for item in items {
        // accept both {key, value} objects and [key, value] pairs.
        let (key, value) = if let Some(object) = item.as_object() {
            let key = object
                .get("key")
                .and_then(Value::as_str)
                .ok_or_else(|| err("from_entries", "entry object needs a string 'key'"))?;
            (
                key.to_string(),
                object.get("value").cloned().unwrap_or(Value::Null),
            )
        } else if let Some(pair) = item.as_array().filter(|pair| pair.len() == 2) {
            let key = pair[0]
                .as_str()
                .ok_or_else(|| err("from_entries", "entry pair key must be a string"))?;
            (key.to_string(), pair[1].clone())
        } else {
            return Err(err(
                "from_entries",
                "entry must be {key, value} or [key, value]",
            ));
        };
        out.insert(key, value);
    }
    Ok(Value::Object(out))
}

fn intrinsic_parse_json(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("parse_json", args, 0)?;
    serde_json::from_str::<serde_json::Value>(&text)
        .map(Value::from)
        .map_err(|e| err("parse_json", e.to_string()))
}

fn intrinsic_base64_encode(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("base64_encode", args, 0)?;
    Ok(Value::String(
        base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
    ))
}

fn intrinsic_base64_decode(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("base64_decode", args, 0)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.as_bytes())
        .map_err(|e| err("base64_decode", e.to_string()))?;
    String::from_utf8(bytes)
        .map(Value::String)
        .map_err(|e| err("base64_decode", e.to_string()))
}

// parse an RFC 3339 timestamp; `now`-style strings produced by the std library round-trip here.
fn parse_rfc3339(
    name: &str,
    text: &str,
) -> Result<chrono::DateTime<chrono::Utc>, WorkflowValidationError> {
    chrono::DateTime::parse_from_rfc3339(text)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .map_err(|e| err(name, format!("invalid RFC 3339 timestamp: {e}")))
}

fn intrinsic_format_date(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let source = arg("format_date", args, 0)?;
    let fmt = string_at("format_date", args, 1)?;
    let datetime = if let Some(epoch) = source.as_i64() {
        chrono::DateTime::from_timestamp(epoch, 0)
            .ok_or_else(|| err("format_date", "epoch seconds out of range"))?
    } else if let Some(text) = source.as_str() {
        parse_rfc3339("format_date", text)?
    } else {
        return Err(err(
            "format_date",
            "first argument must be epoch seconds or an RFC 3339 string",
        ));
    };
    Ok(Value::String(datetime.format(&fmt).to_string()))
}

fn intrinsic_parse_date(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("parse_date", args, 0)?;
    let fmt = string_at("parse_date", args, 1)?;
    // try a full datetime first, then a date-only pattern (assumed midnight UTC).
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(&text, &fmt) {
        let datetime =
            chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc);
        return Ok(Value::String(datetime.to_rfc3339()));
    }
    let date = chrono::NaiveDate::parse_from_str(&text, &fmt)
        .map_err(|e| err("parse_date", format!("could not parse with pattern: {e}")))?;
    let naive = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| err("parse_date", "invalid time"))?;
    let datetime = chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc);
    Ok(Value::String(datetime.to_rfc3339()))
}

fn intrinsic_add_duration(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let datetime = parse_rfc3339("add_duration", &string_at("add_duration", args, 0)?)?;
    let seconds = int_at("add_duration", args, 1)?;
    let shifted = datetime + chrono::Duration::seconds(seconds);
    Ok(Value::String(shifted.to_rfc3339()))
}

fn intrinsic_date_diff(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let left = parse_rfc3339("date_diff", &string_at("date_diff", args, 0)?)?;
    let right = parse_rfc3339("date_diff", &string_at("date_diff", args, 1)?)?;
    Ok(Value::from((left - right).num_seconds()))
}

fn compile_regex(name: &str, pattern: &str) -> Result<regex::Regex, WorkflowValidationError> {
    regex::Regex::new(pattern).map_err(|e| err(name, format!("invalid pattern: {e}")))
}

fn intrinsic_regex_match(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("regex_match", args, 0)?;
    let regex = compile_regex("regex_match", &string_at("regex_match", args, 1)?)?;
    Ok(Value::Bool(regex.is_match(&text)))
}

fn intrinsic_regex_replace(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("regex_replace", args, 0)?;
    let regex = compile_regex("regex_replace", &string_at("regex_replace", args, 1)?)?;
    let replacement = string_at("regex_replace", args, 2)?;
    Ok(Value::String(
        regex.replace_all(&text, replacement.as_str()).into_owned(),
    ))
}

fn intrinsic_regex_extract(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let text = string_at("regex_extract", args, 0)?;
    let regex = compile_regex("regex_extract", &string_at("regex_extract", args, 1)?)?;
    let matches = regex
        .find_iter(&text)
        .map(|m| Value::String(m.as_str().to_string()))
        .collect();
    Ok(Value::Array(matches))
}
