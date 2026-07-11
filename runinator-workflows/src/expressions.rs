use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};

use crate::compute::{cmp_values, is_higher_order};
use crate::errors::WorkflowValidationError;
use crate::functions::{EvalEnv, FunctionTable, invoke_user_function};
use crate::keys::{
    REF_CONFIG, REF_INPUT, REF_LOCAL, REF_OUTPUT, REF_PREV, REF_STEPS, REF_WORKFLOW,
};
use runinator_models::workflow_ast::{
    WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef,
};

/// resolve refs/arithmetic plus pure `$call` intrinsics against `context`. this is the eager
/// reducer path: declarative expressions fold here with the pure standard library, so pure calls
/// (and higher-order `map`/`filter`/...) work outside compute blocks. effectful intrinsics are not
/// in this library and error; the wdl front end already rejects them in declarative positions.
pub fn resolve_value_refs(
    value: &Value,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    resolve_value_refs_with(
        value,
        context,
        EvalEnv::lib_only(Some(&crate::compute::PureIntrinsics)),
    )
}

/// resolve refs/arithmetic plus pure `$call` intrinsics (the std stdlib and higher-order
/// `map`/`filter`/`reduce`/...) against `context`. effectful intrinsics (`http_get`/`now`/`uuid`/...)
/// are not available and error, so this is safe for previews: it evaluates the compute tier without
/// running side effects.
pub fn resolve_value_refs_pure(
    value: &Value,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    resolve_value_refs_with(
        value,
        context,
        EvalEnv::lib_only(Some(&crate::compute::PureIntrinsics)),
    )
}

/// resolve refs/arithmetic, `$call` intrinsics, and user-function calls against `context`, using the
/// library and function table carried by `env`. an `env` with no library forbids `$call` (the
/// library-free path used outside compute blocks); an `env` with no function table forbids user calls.
pub(crate) fn resolve_value_refs_with(
    value: &Value,
    context: &Value,
    env: EvalEnv,
) -> Result<Value, WorkflowValidationError> {
    let expression = parse_expression(value)?;
    evaluate_expression_with(&expression, context, env)
}

/// validate that a value is a well-formed workflow expression without resolving references.
pub fn validate_expression(value: &Value) -> Result<(), WorkflowValidationError> {
    parse_expression(value).map(|_| ())
}

/// like `resolve_value_refs`, but also resolving user-function calls from `functions`. used by the
/// reducer when folding declarative expressions that may reference user-defined functions.
pub fn resolve_value_refs_with_functions(
    value: &Value,
    context: &Value,
    functions: &FunctionTable,
) -> Result<Value, WorkflowValidationError> {
    resolve_value_refs_with(
        value,
        context,
        EvalEnv::new(Some(&crate::compute::PureIntrinsics), Some(functions)),
    )
}

/// fill omitted top-level input fields from their declared defaults, mutating the `input` slot of
/// the run `context` in place. each default is an expression evaluated against the same context, so
/// it may reference `config.*`, `run.*`, `secret.*` (left as `secret://` strings), and sibling
/// input fields. defaults are resolved over repeated passes so one default can read another;
/// provided fields are never overwritten and unresolvable defaults are skipped.
pub fn apply_input_defaults(context: &mut Value, input_type: &RuninatorType) {
    let RuninatorType::Struct { fields, .. } = input_type else {
        return;
    };
    if fields.values().all(|field| field.default.is_none()) {
        return;
    }
    // ensure there is an `input` object to fill; only synthesize one when input is absent/null so a
    // caller-supplied non-object value is never clobbered.
    let needs_object = match context.get(REF_INPUT) {
        Some(value) => value.is_null(),
        None => true,
    };
    if needs_object && let Some(object) = context.as_object_mut() {
        object.insert(REF_INPUT.into(), Value::Object(Map::new()));
    }
    if !context.get(REF_INPUT).is_some_and(Value::is_object) {
        return;
    }
    loop {
        let mut progressed = false;
        for (name, field) in fields {
            let Some(default) = &field.default else {
                continue;
            };
            let present = context
                .get(REF_INPUT)
                .and_then(|input| input.get(name))
                .is_some();
            if present {
                continue;
            }
            let Ok(value) = resolve_value_refs(default, context) else {
                continue;
            };
            if let Some(input) = context.get_mut(REF_INPUT).and_then(Value::as_object_mut) {
                input.insert(name.clone(), value);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
}

// the structural `Value` <-> expression parse and serialize now live in
// `runinator_models::workflow_ast`; these wrappers keep the crate's error type and call sites.

pub(crate) fn parse_expression(
    value: &Value,
) -> Result<WorkflowExpression, WorkflowValidationError> {
    WorkflowExpression::try_from(value)
        .map_err(|err| WorkflowValidationError::InvalidValueRef(err.0))
}

pub(crate) fn evaluate_expression_with(
    expression: &WorkflowExpression,
    context: &Value,
    env: EvalEnv,
) -> Result<Value, WorkflowValidationError> {
    match expression {
        WorkflowExpression::Literal(value) => match value {
            Value::Object(map) => {
                let mut resolved = Map::new();
                for (key, nested) in map {
                    resolved.insert(key.clone(), resolve_value_refs_with(nested, context, env)?);
                }
                Ok(Value::Object(resolved))
            }
            Value::Array(items) => items
                .iter()
                .map(|item| resolve_value_refs_with(item, context, env))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Array),
            _ => Ok(value.clone()),
        },
        WorkflowExpression::Ref(reference) => resolve_value_ref(reference, context),
        WorkflowExpression::Concat(items) => {
            let mut rendered = String::new();
            for item in items {
                let Value::String(value) = evaluate_expression_with(item, context, env)? else {
                    return Err(WorkflowValidationError::InvalidValueRef(
                        "$concat items must resolve to strings".into(),
                    ));
                };
                rendered.push_str(&value);
            }
            Ok(Value::String(rendered))
        }
        WorkflowExpression::Coalesce(items) => {
            for item in items {
                let value = evaluate_expression_with(item, context, env)?;
                if !value.is_null() {
                    return Ok(value);
                }
            }
            Ok(Value::Null)
        }
        WorkflowExpression::ToString(nested) => {
            match evaluate_expression_with(nested, context, env)? {
                Value::String(value) => Ok(Value::String(value)),
                Value::Bool(value) => Ok(Value::String(value.to_string())),
                Value::Number(value) => Ok(Value::String(value.to_string())),
                Value::Null | Value::Array(_) | Value::Object(_) => {
                    Err(WorkflowValidationError::InvalidValueRef(
                        "$to_string requires a string, boolean, or number".into(),
                    ))
                }
            }
        }
        WorkflowExpression::ToJsonString(nested) => {
            let value = evaluate_expression_with(nested, context, env)?;
            if !matches!(value, Value::Array(_) | Value::Object(_)) {
                return Err(WorkflowValidationError::InvalidValueRef(
                    "$to_json_string requires an array or object".into(),
                ));
            }
            // `Value`'s `Display` renders the same compact json; keep the codec out of domain logic.
            Ok(Value::String(value.to_string()))
        }
        WorkflowExpression::Add(items) => fold_arith(
            items,
            context,
            env,
            "+",
            |a, b| Ok(a.wrapping_add(b)),
            |a, b| a + b,
        ),
        WorkflowExpression::Sub(items) => fold_arith(
            items,
            context,
            env,
            "-",
            |a, b| Ok(a.wrapping_sub(b)),
            |a, b| a - b,
        ),
        WorkflowExpression::Mul(items) => fold_arith(
            items,
            context,
            env,
            "*",
            |a, b| Ok(a.wrapping_mul(b)),
            |a, b| a * b,
        ),
        WorkflowExpression::Div(items) => fold_arith(
            items,
            context,
            env,
            "/",
            |a, b| {
                a.checked_div(b).ok_or_else(|| {
                    WorkflowValidationError::InvalidValueRef("division by zero".into())
                })
            },
            |a, b| a / b,
        ),
        WorkflowExpression::Mod(items) => fold_arith(
            items,
            context,
            env,
            "%",
            |a, b| {
                a.checked_rem(b).ok_or_else(|| {
                    WorkflowValidationError::InvalidValueRef("modulo by zero".into())
                })
            },
            |a, b| a % b,
        ),
        WorkflowExpression::Neg(nested) => {
            match as_number(&evaluate_expression_with(nested, context, env)?)? {
                Num::Int(value) => Ok(Value::from(value.wrapping_neg())),
                Num::Float(value) => Ok(float_value(-value)?),
            }
        }
        WorkflowExpression::Call { name, args } => {
            // higher-order intrinsics need the evaluator and context to apply their lambda, so the
            // engine handles them directly rather than dispatching through the library.
            if is_higher_order(name) {
                return evaluate_higher_order(name, args, context, env);
            }
            // a user-defined function: evaluate its args in this context, then apply the body.
            if let Some(function) = env.lookup(name) {
                let values = args
                    .iter()
                    .map(|arg| evaluate_expression_with(arg, context, env))
                    .collect::<Result<Vec<_>, _>>()?;
                return invoke_user_function(name, function, &values, env);
            }
            let lib = env.lib.ok_or_else(|| {
                WorkflowValidationError::InvalidValueRef(format!(
                    "call to '{name}' is not allowed in this context"
                ))
            })?;
            let args = args
                .iter()
                .map(|arg| evaluate_expression_with(arg, context, env))
                .collect::<Result<Vec<_>, _>>()?;
            lib.call(name, &args)
        }
        // evaluate the condition, then only the taken branch — keeping recursion base cases lazy.
        WorkflowExpression::Cond {
            condition,
            then,
            otherwise,
        } => {
            let taken = if is_truthy(&evaluate_expression_with(condition, context, env)?) {
                then
            } else {
                otherwise
            };
            evaluate_expression_with(taken, context, env)
        }
        // a lambda has no standalone value; it is only meaningful as a higher-order argument.
        WorkflowExpression::Lambda { .. } => Err(WorkflowValidationError::InvalidValueRef(
            "a lambda may only be passed to a higher-order intrinsic".into(),
        )),
    }
}

/// truthiness for a conditional expression: everything is truthy except null, `false`, zero, the
/// empty string, and empty collections.
fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

/// evaluate a higher-order intrinsic (`map`/`filter`/`reduce`/...). the collection argument is
/// evaluated eagerly; the lambda argument stays an expression whose body is applied per element with
/// its parameters bound into a fresh `let` scope.
fn evaluate_higher_order(
    name: &str,
    args: &[WorkflowExpression],
    context: &Value,
    env: EvalEnv,
) -> Result<Value, WorkflowValidationError> {
    let arg = |index: usize| {
        args.get(index).ok_or_else(|| {
            WorkflowValidationError::InvalidValueRef(format!("'{name}' is missing an argument"))
        })
    };
    // reduce takes (collection, initial, lambda); every other higher-order takes (collection, lambda).
    let lambda_index = if name == "reduce" { 2 } else { 1 };
    let items = collection(name, evaluate_expression_with(arg(0)?, context, env)?)?;
    let (params, body) = as_lambda(name, arg(lambda_index)?)?;

    match name {
        "map" => {
            let mapped = items
                .iter()
                .map(|item| apply_lambda(params, body, &[item.clone()], context, env))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Array(mapped))
        }
        "flat_map" => {
            let mut out = Vec::new();
            for item in &items {
                match apply_lambda(params, body, &[item.clone()], context, env)? {
                    Value::Array(inner) => out.extend(inner),
                    other => out.push(other),
                }
            }
            Ok(Value::Array(out))
        }
        "filter" => {
            let mut out = Vec::new();
            for item in items {
                if predicate(
                    name,
                    apply_lambda(params, body, &[item.clone()], context, env)?,
                )? {
                    out.push(item);
                }
            }
            Ok(Value::Array(out))
        }
        "find" => {
            for item in items {
                if predicate(
                    name,
                    apply_lambda(params, body, &[item.clone()], context, env)?,
                )? {
                    return Ok(item);
                }
            }
            Ok(Value::Null)
        }
        "any" => {
            for item in &items {
                if predicate(
                    name,
                    apply_lambda(params, body, &[item.clone()], context, env)?,
                )? {
                    return Ok(Value::Bool(true));
                }
            }
            Ok(Value::Bool(false))
        }
        "all" => {
            for item in &items {
                if !predicate(
                    name,
                    apply_lambda(params, body, &[item.clone()], context, env)?,
                )? {
                    return Ok(Value::Bool(false));
                }
            }
            Ok(Value::Bool(true))
        }
        "sort_by" => {
            // compute each element's key once, then sort element/key pairs by the key.
            let mut keyed = items
                .into_iter()
                .map(|item| {
                    let key = apply_lambda(params, body, &[item.clone()], context, env)?;
                    Ok((key, item))
                })
                .collect::<Result<Vec<_>, WorkflowValidationError>>()?;
            keyed.sort_by(|(a, _), (b, _)| cmp_values(a, b));
            Ok(Value::Array(
                keyed.into_iter().map(|(_, item)| item).collect(),
            ))
        }
        "reduce" => {
            let mut acc = evaluate_expression_with(arg(1)?, context, env)?;
            for item in items {
                acc = apply_lambda(params, body, &[acc, item], context, env)?;
            }
            Ok(acc)
        }
        _ => Err(WorkflowValidationError::InvalidValueRef(format!(
            "unknown higher-order intrinsic '{name}'"
        ))),
    }
}

/// extract a lambda's parameters and body, rejecting a non-lambda argument.
fn as_lambda<'a>(
    name: &str,
    expr: &'a WorkflowExpression,
) -> Result<(&'a [String], &'a WorkflowExpression), WorkflowValidationError> {
    match expr {
        WorkflowExpression::Lambda { params, body } => Ok((params, body)),
        _ => Err(WorkflowValidationError::InvalidValueRef(format!(
            "'{name}' requires a lambda argument"
        ))),
    }
}

/// require an array, naming the offending intrinsic.
fn collection(name: &str, value: Value) -> Result<Vec<Value>, WorkflowValidationError> {
    match value {
        Value::Array(items) => Ok(items),
        other => Err(WorkflowValidationError::InvalidValueRef(format!(
            "'{name}' requires an array, got {other}"
        ))),
    }
}

/// require a boolean lambda result for the predicate-style higher-order intrinsics.
fn predicate(name: &str, value: Value) -> Result<bool, WorkflowValidationError> {
    match value {
        Value::Bool(value) => Ok(value),
        other => Err(WorkflowValidationError::InvalidValueRef(format!(
            "'{name}' lambda must return a boolean, got {other}"
        ))),
    }
}

/// apply a lambda body with `values` bound to its parameters in a fresh `let` scope layered over a
/// clone of `context`.
fn apply_lambda(
    params: &[String],
    body: &WorkflowExpression,
    values: &[Value],
    context: &Value,
    env: EvalEnv,
) -> Result<Value, WorkflowValidationError> {
    let mut scope = context.clone();
    if !scope.get(REF_LOCAL).is_some_and(Value::is_object)
        && let Some(object) = scope.as_object_mut()
    {
        object.insert(REF_LOCAL.into(), Value::Object(Map::new()));
    }
    for (param, value) in params.iter().zip(values.iter()) {
        if let Some(locals) = scope.get_mut(REF_LOCAL).and_then(Value::as_object_mut) {
            locals.insert(param.clone(), value.clone());
        }
    }
    evaluate_expression_with(body, &scope, env)
}

#[derive(Clone, Copy)]
enum Num {
    Int(i64),
    Float(f64),
}

fn as_number(value: &Value) -> Result<Num, WorkflowValidationError> {
    if let Some(int) = value.as_i64() {
        return Ok(Num::Int(int));
    }
    if let Some(float) = value.as_f64() {
        return Ok(Num::Float(float));
    }
    Err(WorkflowValidationError::InvalidValueRef(format!(
        "arithmetic requires numbers, got {value}"
    )))
}

fn float_value(value: f64) -> Result<Value, WorkflowValidationError> {
    if value.is_finite() {
        return Ok(Value::from(value));
    }
    Err(WorkflowValidationError::InvalidValueRef(
        "arithmetic produced a non-finite number".into(),
    ))
}

// fold operands left-to-right, staying in integer space while every operand is an integer and
// promoting to float as soon as any operand is a float.
fn fold_arith(
    items: &[WorkflowExpression],
    context: &Value,
    env: EvalEnv,
    op: &str,
    int_op: impl Fn(i64, i64) -> Result<i64, WorkflowValidationError>,
    float_op: impl Fn(f64, f64) -> f64,
) -> Result<Value, WorkflowValidationError> {
    let mut iter = items.iter();
    let first = iter.next().ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef(format!("'{op}' requires at least one operand"))
    })?;
    let mut acc = as_number(&evaluate_expression_with(first, context, env)?)?;
    for item in iter {
        let next = as_number(&evaluate_expression_with(item, context, env)?)?;
        acc = match (acc, next) {
            (Num::Int(a), Num::Int(b)) => Num::Int(int_op(a, b)?),
            (a, b) => Num::Float(float_op(num_as_f64(a), num_as_f64(b))),
        };
    }
    match acc {
        Num::Int(value) => Ok(Value::from(value)),
        Num::Float(value) => float_value(value),
    }
}

fn num_as_f64(value: Num) -> f64 {
    match value {
        Num::Int(value) => value as f64,
        Num::Float(value) => value,
    }
}

pub(crate) fn parse_value_ref(value: &Value) -> Result<WorkflowValueRef, WorkflowValidationError> {
    WorkflowValueRef::try_from(value).map_err(|err| WorkflowValidationError::InvalidValueRef(err.0))
}

pub(crate) fn resolve_value_ref(
    reference: &WorkflowValueRef,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    // a node ref resolves into the node's `output_json` first; if that path misses, it falls back
    // to the step root so siblings of `output` (notably `artifacts`) are reachable via the same
    // `node.field` surface. providers that put a real key in `output` always win the lookup.
    if let WorkflowRefSource::NodeOutput(node) = &reference.source {
        let step = context
            .get(REF_STEPS)
            .and_then(|steps| steps.get(node.as_str()))
            .ok_or_else(|| {
                WorkflowValidationError::InvalidValueRef(serialize_value_ref(reference).to_string())
            })?;
        let from_output = step
            .get(REF_OUTPUT)
            .and_then(|output| resolve_path(output, &reference.path));
        let resolved = match from_output {
            Some(value) => value.clone(),
            None => resolve_path(step, &reference.path)
                .cloned()
                .unwrap_or(Value::Null),
        };
        return Ok(resolved);
    }
    let base = match &reference.source {
        WorkflowRefSource::Input => context.get(REF_INPUT),
        WorkflowRefSource::Prev => context.get(REF_PREV),
        WorkflowRefSource::Workflow => context.get(REF_WORKFLOW),
        WorkflowRefSource::Config => context.get(REF_CONFIG),
        WorkflowRefSource::Local => context.get(REF_LOCAL),
        // node-output refs are resolved before this point; treat any that reach here as unresolved
        // rather than panicking, so a malformed reference surfaces as a validation error.
        WorkflowRefSource::NodeOutput(_) => None,
    }
    .ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef(serialize_value_ref(reference).to_string())
    })?;
    Ok(resolve_path(base, &reference.path)
        .cloned()
        .unwrap_or(Value::Null))
}

pub(crate) fn resolve_path<'a>(
    value: &'a Value,
    path: &[WorkflowPathSegment],
) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = match segment {
            WorkflowPathSegment::Key(key) => current.get(key)?,
            WorkflowPathSegment::Index(index) => current.get(*index)?,
        };
    }
    Some(current)
}

pub(crate) fn serialize_value_ref(reference: &WorkflowValueRef) -> Value {
    Value::from(reference)
}
