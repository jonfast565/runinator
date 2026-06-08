use runinator_models::providers::{ActionMetadata, ParameterMetadata, ResultMetadata};
use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};

use crate::conditions::evaluate_condition_with;
use crate::errors::WorkflowValidationError;
use crate::expressions::{evaluate_expression_with, parse_expression};
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
    let mut working = context.clone();
    ensure_local_slot(&mut working);
    match run_block(&program.0, &mut working, lib)? {
        Some(outcome) => Ok(outcome),
        None => Ok(ComputeOutcome::Fallthrough(Value::Null)),
    }
}

fn run_block(
    statements: &[ComputeStmt],
    context: &mut Value,
    lib: &dyn IntrinsicLibrary,
) -> Result<Option<ComputeOutcome>, WorkflowValidationError> {
    for statement in statements {
        match statement {
            ComputeStmt::Let { name, value } => {
                let evaluated = evaluate_expression_with(value, context, Some(lib))?;
                set_local(context, name, evaluated);
            }
            ComputeStmt::Return(expr) => {
                let value = evaluate_expression_with(expr, context, Some(lib))?;
                return Ok(Some(ComputeOutcome::Return(value)));
            }
            ComputeStmt::Goto(target) => {
                return Ok(Some(ComputeOutcome::Goto(target.clone())));
            }
            ComputeStmt::Expr(expr) => {
                evaluate_expression_with(expr, context, Some(lib))?;
            }
            ComputeStmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let branch = if evaluate_condition_with(condition, context, lib)? {
                    &then_branch.0
                } else {
                    &else_branch.0
                };
                if let Some(outcome) = run_block(branch, context, lib)? {
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
            "add",
            "sub",
            "mul",
            "div",
            "mod",
            "len",
            "keys",
            "lower",
            "upper",
            "floor",
            "ceil",
            "round",
            "min",
            "max",
            "parse_int",
            "parse_float",
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

/// whether `name` is any known intrinsic (pure or effectful).
pub fn is_known_intrinsic(name: &str) -> bool {
    PureIntrinsics::contains(name) || EFFECTFUL_INTRINSIC_NAMES.contains(&name)
}

/// the accepted argument count range `(min, max)` for an intrinsic, or `None` if unknown. used by
/// sema to flag obvious arity mistakes (a typo'd name returns `None` and is rejected separately).
pub fn intrinsic_arity(name: &str) -> Option<(usize, usize)> {
    let arity = match name {
        "add" | "sub" | "mul" | "div" | "mod" | "min" | "max" => (2, 2),
        "len" | "keys" | "lower" | "upper" | "floor" | "ceil" | "round" | "parse_int"
        | "parse_float" => (1, 1),
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
        "floor" => number_to_int(name, args, f64::floor),
        "ceil" => number_to_int(name, args, f64::ceil),
        "round" => number_to_int(name, args, f64::round),
        "parse_int" => parse_int(args),
        "parse_float" => parse_float(args),
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
