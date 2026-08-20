//! the surface operators the author-facing library does not name.
//!
//! `a ++ b`, `string(x)`, `json(x)`, unary `-`, and the truthiness/existence tests a declarative
//! condition performs are all things the language has and `std` does not: an author writes `++`,
//! never `concat(...)`. The VM represents them as intrinsic calls rather than dedicated opcodes.
//!
//! so they are calls here, under `$`-prefixed names that cannot collide with a library function or
//! a user `fn`. the implementations are transcribed from the evaluator's arms rather than reimagined
//! — `operators_tests.rs` asserts the two agree on the cases where they could plausibly drift
//! (integer vs float arithmetic, what counts as truthy, what `string()` refuses).

use runinator_models::value::Value;

use crate::errors::WorkflowValidationError;

/// whether a name is one of the assembler-emitted operator intrinsics.
pub fn is_operator_intrinsic(name: &str) -> bool {
    crate::assemble::OPERATOR_INTRINSICS.contains(&name)
}

/// apply an operator intrinsic to already-evaluated operands.
pub fn call_operator(name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError> {
    match name {
        crate::assemble::CONCAT_INTRINSIC => concat(args),
        crate::assemble::TO_STRING_INTRINSIC => to_string(unary(name, args)?),
        crate::assemble::TO_JSON_INTRINSIC => to_json(unary(name, args)?),
        crate::assemble::NEG_INTRINSIC => neg(unary(name, args)?),
        crate::assemble::IS_NULL_INTRINSIC => Ok(Value::Bool(unary(name, args)?.is_null())),
        crate::assemble::TRUTHY_INTRINSIC => Ok(Value::Bool(truthy(unary(name, args)?))),
        crate::assemble::NOT_INTRINSIC => Ok(Value::Bool(!truthy(unary(name, args)?))),
        crate::assemble::EXISTS_INTRINSIC => Ok(Value::Bool(!unary(name, args)?.is_null())),
        crate::assemble::IN_INTRINSIC => contains_reversed(name, args),
        _ => Err(WorkflowValidationError::InvalidComputeProgram(format!(
            "'{name}' is not an operator intrinsic"
        ))),
    }
}

fn unary<'a>(name: &str, args: &'a [Value]) -> Result<&'a Value, WorkflowValidationError> {
    match args {
        [only] => Ok(only),
        _ => Err(WorkflowValidationError::InvalidComputeProgram(format!(
            "'{name}' takes exactly one argument but got {}",
            args.len()
        ))),
    }
}

// transcribed from `WorkflowExpression::Concat`: every operand must already be a string.
fn concat(args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let mut rendered = String::new();
    for item in args {
        let Value::String(value) = item else {
            return Err(WorkflowValidationError::InvalidValueRef(
                "$concat items must resolve to strings".into(),
            ));
        };
        rendered.push_str(value);
    }
    Ok(Value::String(rendered))
}

// transcribed from `WorkflowExpression::ToString`.
fn to_string(value: &Value) -> Result<Value, WorkflowValidationError> {
    match value {
        Value::String(value) => Ok(Value::String(value.clone())),
        Value::Bool(value) => Ok(Value::String(value.to_string())),
        Value::Number(value) => Ok(Value::String(value.to_string())),
        Value::Null | Value::Array(_) | Value::Object(_) => {
            Err(WorkflowValidationError::InvalidValueRef(
                "$to_string requires a string, boolean, or number".into(),
            ))
        }
    }
}

// transcribed from `WorkflowExpression::ToJsonString`.
fn to_json(value: &Value) -> Result<Value, WorkflowValidationError> {
    if !matches!(value, Value::Array(_) | Value::Object(_)) {
        return Err(WorkflowValidationError::InvalidValueRef(
            "$to_json_string requires an array or object".into(),
        ));
    }
    Ok(Value::String(value.to_string()))
}

// transcribed from `WorkflowExpression::Neg`: integers wrap, floats negate.
fn neg(value: &Value) -> Result<Value, WorkflowValidationError> {
    let Value::Number(number) = value else {
        return Err(WorkflowValidationError::InvalidValueRef(
            "'-' requires a number".into(),
        ));
    };
    match number.as_i64() {
        Some(integer) => Ok(Value::from(integer.wrapping_neg())),
        None => {
            let float = number.as_f64().ok_or_else(|| {
                WorkflowValidationError::InvalidValueRef("'-' requires a number".into())
            })?;
            Ok(Value::from(-float))
        }
    }
}

/// the declarative-condition truthiness rule, shared by conditional-expression bytecode.
///
/// deliberately not restated here. a `{value: x}` condition decides which branch a workflow takes,
/// and a second definition of "truthy" is how that decision quietly stops matching the one the
/// evaluator made for the same program.
fn truthy(value: &Value) -> bool {
    crate::conditions::is_truthy(value)
}

// `left in right` is `contains(right, left)`; the assembler pushes left then right, so the operands
// are swapped here rather than at the call site, where the swap would be invisible in the bytecode.
fn contains_reversed(name: &str, args: &[Value]) -> Result<Value, WorkflowValidationError> {
    let [left, right] = args else {
        return Err(WorkflowValidationError::InvalidComputeProgram(format!(
            "'{name}' takes exactly two arguments but got {}",
            args.len()
        )));
    };
    crate::compute::call_pure("contains", &[right.clone(), left.clone()])
}

#[cfg(test)]
#[path = "operators_tests.rs"]
mod tests;
