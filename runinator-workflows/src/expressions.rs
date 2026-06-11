use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};
use runinator_models::workflows::WorkflowNodeRef;

use crate::compute::{cmp_values, is_higher_order};
use crate::errors::WorkflowValidationError;
use crate::functions::{EvalEnv, FunctionTable, invoke_user_function};
use crate::keys::{
    EXPR_ADD, EXPR_ARGS, EXPR_CALL, EXPR_COALESCE, EXPR_CONCAT, EXPR_DIV, EXPR_ELSE, EXPR_IF,
    EXPR_LAMBDA, EXPR_LITERAL, EXPR_MOD, EXPR_MUL, EXPR_NEG, EXPR_NODE, EXPR_REF, EXPR_SUB,
    EXPR_THEN, EXPR_TO_JSON_STRING, EXPR_TO_STRING, EXPR_VALUE, LAMBDA_BODY, LAMBDA_PARAMS,
    REF_CONFIG, REF_INPUT, REF_LOCAL, REF_NODE, REF_OUTPUT, REF_PARAMS, REF_PREV, REF_STEPS,
    REF_WORKFLOW,
};
use crate::types::{WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef};

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

pub(crate) fn parse_expression(
    value: &Value,
) -> Result<WorkflowExpression, WorkflowValidationError> {
    match value {
        Value::Object(map) if map.contains_key(EXPR_VALUE) => {
            Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
        }
        Value::Object(map) if map.contains_key(EXPR_CALL) => {
            let name = map
                .get(EXPR_CALL)
                .and_then(Value::as_str)
                .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
            let allowed = map.keys().all(|key| key == EXPR_CALL || key == EXPR_ARGS);
            if !allowed {
                return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            let args = match map.get(EXPR_ARGS) {
                None => Vec::new(),
                Some(items) => items
                    .as_array()
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?
                    .iter()
                    .map(parse_expression)
                    .collect::<Result<Vec<_>, _>>()?,
            };
            Ok(WorkflowExpression::Call {
                name: name.to_string(),
                args,
            })
        }
        Value::Object(map) if map.contains_key(EXPR_LAMBDA) => {
            if map.len() != 1 {
                return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            let spec = map
                .get(EXPR_LAMBDA)
                .and_then(Value::as_object)
                .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
            let params =
                spec.get(LAMBDA_PARAMS)
                    .and_then(Value::as_array)
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?
                    .iter()
                    .map(|param| {
                        param.as_str().map(str::to_string).ok_or_else(|| {
                            WorkflowValidationError::InvalidValueRef(value.to_string())
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
            let body = spec
                .get(LAMBDA_BODY)
                .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
            Ok(WorkflowExpression::Lambda {
                params,
                body: Box::new(parse_expression(body)?),
            })
        }
        Value::Object(map) if map.contains_key(EXPR_IF) => {
            let allowed = map
                .keys()
                .all(|key| key == EXPR_IF || key == EXPR_THEN || key == EXPR_ELSE);
            if !allowed {
                return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            let branch = |key: &str| {
                map.get(key)
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))
                    .and_then(parse_expression)
                    .map(Box::new)
            };
            Ok(WorkflowExpression::Cond {
                condition: Box::new(parse_expression(map.get(EXPR_IF).expect("checked above"))?),
                then: branch(EXPR_THEN)?,
                otherwise: branch(EXPR_ELSE)?,
            })
        }
        Value::Object(map)
            if map.contains_key(EXPR_REF)
                || map.contains_key(EXPR_CONCAT)
                || map.contains_key(EXPR_COALESCE)
                || map.contains_key(EXPR_LITERAL)
                || map.contains_key(EXPR_TO_STRING)
                || map.contains_key(EXPR_TO_JSON_STRING)
                || map.contains_key(EXPR_ADD)
                || map.contains_key(EXPR_SUB)
                || map.contains_key(EXPR_MUL)
                || map.contains_key(EXPR_DIV)
                || map.contains_key(EXPR_MOD)
                || map.contains_key(EXPR_NEG)
                || map.contains_key(EXPR_NODE) =>
        {
            if map.len() != 1 {
                return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            if let Some(reference) = map.get(EXPR_REF) {
                return Ok(WorkflowExpression::Ref(parse_value_ref(reference)?));
            }
            for (key, ctor) in [
                (EXPR_ADD, WorkflowExpression::Add as fn(_) -> _),
                (EXPR_SUB, WorkflowExpression::Sub),
                (EXPR_MUL, WorkflowExpression::Mul),
                (EXPR_DIV, WorkflowExpression::Div),
                (EXPR_MOD, WorkflowExpression::Mod),
            ] {
                if let Some(items) = map.get(key) {
                    let items = items
                        .as_array()
                        .filter(|items| !items.is_empty())
                        .ok_or_else(|| {
                            WorkflowValidationError::InvalidValueRef(value.to_string())
                        })?;
                    return Ok(ctor(
                        items
                            .iter()
                            .map(parse_expression)
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
            }
            if let Some(operand) = map.get(EXPR_NEG) {
                return Ok(WorkflowExpression::Neg(Box::new(parse_expression(
                    operand,
                )?)));
            }
            if let Some(items) = map.get(EXPR_CONCAT) {
                let items = items
                    .as_array()
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
                return Ok(WorkflowExpression::Concat(
                    items
                        .iter()
                        .map(parse_expression)
                        .collect::<Result<Vec<_>, _>>()?,
                ));
            }
            if let Some(items) = map.get(EXPR_COALESCE) {
                let items = items
                    .as_array()
                    .filter(|items| !items.is_empty())
                    .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
                return Ok(WorkflowExpression::Coalesce(
                    items
                        .iter()
                        .map(parse_expression)
                        .collect::<Result<Vec<_>, _>>()?,
                ));
            }
            if let Some(literal) = map.get(EXPR_LITERAL) {
                return Ok(WorkflowExpression::Literal(literal.clone()));
            }
            if let Some(nested) = map.get(EXPR_TO_STRING) {
                return Ok(WorkflowExpression::ToString(Box::new(parse_expression(
                    nested,
                )?)));
            }
            if let Some(nested) = map.get(EXPR_TO_JSON_STRING) {
                return Ok(WorkflowExpression::ToJsonString(Box::new(
                    parse_expression(nested)?,
                )));
            }
            Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
        }
        Value::Object(map) => {
            let mut resolved = Map::new();
            for (key, nested) in map {
                resolved.insert(
                    key.clone(),
                    evaluate_static_expression(parse_expression(nested)?)?,
                );
            }
            Ok(WorkflowExpression::Literal(Value::Object(resolved)))
        }
        Value::Array(items) => Ok(WorkflowExpression::Literal(Value::Array(
            items
                .iter()
                .map(|item| evaluate_static_expression(parse_expression(item)?))
                .collect::<Result<Vec<_>, _>>()?,
        ))),
        Value::String(raw) if raw.contains("{{") || raw.contains("}}") => {
            Err(WorkflowValidationError::InvalidValueRef(raw.clone()))
        }
        _ => Ok(WorkflowExpression::Literal(value.clone())),
    }
}

pub(crate) fn evaluate_static_expression(
    expression: WorkflowExpression,
) -> Result<Value, WorkflowValidationError> {
    match expression {
        WorkflowExpression::Literal(value) => Ok(value),
        WorkflowExpression::Ref(reference) => Ok(Value::Object(Map::from_iter([(
            EXPR_REF.into(),
            serialize_value_ref(&reference),
        )]))),
        WorkflowExpression::Concat(items) => Ok(Value::Object(Map::from_iter([(
            EXPR_CONCAT.into(),
            Value::Array(
                items
                    .into_iter()
                    .map(evaluate_static_expression)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        )]))),
        WorkflowExpression::Coalesce(items) => Ok(Value::Object(Map::from_iter([(
            EXPR_COALESCE.into(),
            Value::Array(
                items
                    .into_iter()
                    .map(evaluate_static_expression)
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        )]))),
        WorkflowExpression::ToString(nested) => Ok(Value::Object(Map::from_iter([(
            EXPR_TO_STRING.into(),
            evaluate_static_expression(*nested)?,
        )]))),
        WorkflowExpression::ToJsonString(nested) => Ok(Value::Object(Map::from_iter([(
            EXPR_TO_JSON_STRING.into(),
            evaluate_static_expression(*nested)?,
        )]))),
        WorkflowExpression::Add(items) => static_arith(EXPR_ADD, items),
        WorkflowExpression::Sub(items) => static_arith(EXPR_SUB, items),
        WorkflowExpression::Mul(items) => static_arith(EXPR_MUL, items),
        WorkflowExpression::Div(items) => static_arith(EXPR_DIV, items),
        WorkflowExpression::Mod(items) => static_arith(EXPR_MOD, items),
        WorkflowExpression::Neg(nested) => Ok(Value::Object(Map::from_iter([(
            EXPR_NEG.into(),
            evaluate_static_expression(*nested)?,
        )]))),
        WorkflowExpression::Call { name, args } => Ok(Value::Object(Map::from_iter([
            (EXPR_CALL.into(), Value::String(name)),
            (
                EXPR_ARGS.into(),
                Value::Array(
                    args.into_iter()
                        .map(evaluate_static_expression)
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            ),
        ]))),
        WorkflowExpression::Lambda { params, body } => Ok(Value::Object(Map::from_iter([(
            EXPR_LAMBDA.into(),
            Value::Object(Map::from_iter([
                (
                    LAMBDA_PARAMS.into(),
                    Value::Array(params.into_iter().map(Value::String).collect()),
                ),
                (LAMBDA_BODY.into(), evaluate_static_expression(*body)?),
            ])),
        )]))),
        WorkflowExpression::Cond {
            condition,
            then,
            otherwise,
        } => Ok(Value::Object(Map::from_iter([
            (EXPR_IF.into(), evaluate_static_expression(*condition)?),
            (EXPR_THEN.into(), evaluate_static_expression(*then)?),
            (EXPR_ELSE.into(), evaluate_static_expression(*otherwise)?),
        ]))),
    }
}

fn static_arith(
    key: &str,
    items: Vec<WorkflowExpression>,
) -> Result<Value, WorkflowValidationError> {
    Ok(Value::Object(Map::from_iter([(
        key.into(),
        Value::Array(
            items
                .into_iter()
                .map(evaluate_static_expression)
                .collect::<Result<Vec<_>, _>>()?,
        ),
    )])))
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
            serde_json::to_string(&value)
                .map(Value::String)
                .map_err(|err| WorkflowValidationError::InvalidValueRef(err.to_string()))
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
    let object = value
        .as_object()
        .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
    if object.len() != 1
        && !(object.len() == 2 && object.contains_key(REF_NODE) && object.contains_key(REF_OUTPUT))
    {
        return Err(WorkflowValidationError::InvalidValueRef(value.to_string()));
    }
    if let Some(path) = object.get(REF_PARAMS) {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Input,
            path: parse_path(path)?,
        });
    }
    if let Some(path) = object.get(REF_PREV) {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Prev,
            path: parse_path(path)?,
        });
    }
    if let Some(path) = object.get(REF_WORKFLOW) {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Workflow,
            path: parse_path(path)?,
        });
    }
    if let Some(path) = object.get(REF_CONFIG) {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Config,
            path: parse_path(path)?,
        });
    }
    if let Some(path) = object.get(REF_LOCAL) {
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::Local,
            path: parse_path(path)?,
        });
    }
    if let (Some(node), Some(output)) = (object.get(REF_NODE), object.get(REF_OUTPUT)) {
        let node = node
            .as_str()
            .filter(|node| !node.is_empty())
            .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
        return Ok(WorkflowValueRef {
            source: WorkflowRefSource::NodeOutput(WorkflowNodeRef::new(node)),
            path: parse_path(output)?,
        });
    }
    Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
}

pub(crate) fn parse_path(
    value: &Value,
) -> Result<Vec<WorkflowPathSegment>, WorkflowValidationError> {
    let items = value
        .as_array()
        .ok_or_else(|| WorkflowValidationError::InvalidValueRef(value.to_string()))?;
    items
        .iter()
        .map(|item| {
            if let Some(key) = item.as_str() {
                return Ok(WorkflowPathSegment::Key(key.to_string()));
            }
            if let Some(index) = item.as_u64() {
                return usize::try_from(index)
                    .map(WorkflowPathSegment::Index)
                    .map_err(|_| WorkflowValidationError::InvalidValueRef(value.to_string()));
            }
            Err(WorkflowValidationError::InvalidValueRef(value.to_string()))
        })
        .collect()
}

pub(crate) fn resolve_value_ref(
    reference: &WorkflowValueRef,
    context: &Value,
) -> Result<Value, WorkflowValidationError> {
    let base = match &reference.source {
        WorkflowRefSource::Input => context.get(REF_INPUT),
        WorkflowRefSource::Prev => context.get(REF_PREV),
        WorkflowRefSource::Workflow => context.get(REF_WORKFLOW),
        WorkflowRefSource::Config => context.get(REF_CONFIG),
        WorkflowRefSource::Local => context.get(REF_LOCAL),
        WorkflowRefSource::NodeOutput(node) => context
            .get(REF_STEPS)
            .and_then(|steps| steps.get(node.as_str()))
            .and_then(|step| step.get(REF_OUTPUT)),
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
    let path = Value::Array(
        reference
            .path
            .iter()
            .map(|segment| match segment {
                WorkflowPathSegment::Key(key) => Value::String(key.clone()),
                WorkflowPathSegment::Index(index) => Value::from(*index),
            })
            .collect(),
    );
    match &reference.source {
        WorkflowRefSource::Input => runinator_models::json!({ (REF_PARAMS): path }),
        WorkflowRefSource::Prev => runinator_models::json!({ (REF_PREV): path }),
        WorkflowRefSource::Workflow => runinator_models::json!({ (REF_WORKFLOW): path }),
        WorkflowRefSource::Config => runinator_models::json!({ (REF_CONFIG): path }),
        WorkflowRefSource::Local => runinator_models::json!({ (REF_LOCAL): path }),
        WorkflowRefSource::NodeOutput(node) => {
            runinator_models::json!({ (REF_NODE): node.as_str(), (REF_OUTPUT): path })
        }
    }
}
