// user-defined functions and the generated intrinsic catalog.
//
// the catalog is generated from the rust intrinsic metadata (`compute.rs`) so the rexrap front end's
// view of the callable signatures cannot drift from what the runtime actually dispatches. user
// functions are carried in the workflow definition (`metadata.functions`) and evaluated by the
// expression engine: their bodies are hermetic single expressions over their parameters, applied by
// binding arguments into the `let` slot of a fresh scope. recursion is bounded by a per-function
// `max_depth` plus a global safety cap.

use std::collections::HashMap;

use runinator_models::providers::{ActionMetadata, ParameterMetadata, ResultMetadata};
use runinator_models::types::RuninatorType;
use runinator_models::value::Value;

use crate::assemble::{assemble_expression, assemble_module};
use crate::catalog::CallableCatalog;
use crate::compute::{PureIntrinsics, effectful_signatures, parse_program};
use crate::errors::WorkflowValidationError;
use crate::expressions::parse_expression;
use runinator_models::invocation::InvocationModule;
use runinator_models::workflow_ast::{ComputeProgram, WorkflowExpression};

/// every intrinsic's typed signature, generated from the rust metadata. the rexrap front end consumes
/// this as its callable catalog (the "prelude"), so names/arity/types stay in lockstep with the
/// runtime dispatch.
pub fn intrinsic_catalog() -> Vec<ActionMetadata> {
    PureIntrinsics::signatures()
        .into_iter()
        .chain(effectful_signatures())
        .chain(higher_order_signatures())
        .collect()
}

/// signatures for the higher-order intrinsics, which the engine evaluates directly (so they have no
/// entry in `PureIntrinsics::signatures`). typed permissively: the lambda argument is `any`.
fn higher_order_signatures() -> Vec<ActionMetadata> {
    let any_array = || RuninatorType::array(RuninatorType::Any);
    let collection_lambda = |name: &str, result: RuninatorType| {
        ActionMetadata::new(name, format!("higher-order intrinsic {name}"))
            .with_parameters(vec![
                ParameterMetadata::required("collection", any_array()),
                ParameterMetadata::required("f", RuninatorType::Any),
            ])
            .with_results(vec![ResultMetadata::new("result", result)])
            .pure()
    };
    vec![
        collection_lambda("map", any_array()),
        collection_lambda("flat_map", any_array()),
        collection_lambda("filter", any_array()),
        collection_lambda("find", RuninatorType::Any),
        collection_lambda("any", RuninatorType::Boolean),
        collection_lambda("all", RuninatorType::Boolean),
        collection_lambda("sort_by", any_array()),
        ActionMetadata::new("reduce", "higher-order intrinsic reduce")
            .with_parameters(vec![
                ParameterMetadata::required("collection", any_array()),
                ParameterMetadata::required("initial", RuninatorType::Any),
                ParameterMetadata::required("f", RuninatorType::Any),
            ])
            .with_results(vec![ResultMetadata::new("result", RuninatorType::Any)])
            .pure(),
    ]
}

/// a user-defined function body: a single lowered expression, or a compute-style program (the same
/// `$let`/`$return`/`$if` form a `compute` block lowers to) evaluated by the shared block runner.
pub enum FunctionBody {
    Expr(WorkflowExpression),
    Program(ComputeProgram),
}

/// a user-defined function resolved for runtime evaluation: parameter names (binding is positional),
/// the lowered body, and an optional recursion depth limit.

/// parse one `metadata.functions` entry into a `RuntimeFunction`.
fn parse_function(value: &Value) -> Result<RuntimeFunction, WorkflowValidationError> {
    let object = value.as_object().ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef("function must be an object".into())
    })?;
    let params = object
        .get("params")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(param_name).collect::<Result<Vec<_>, _>>())
        .transpose()?
        .unwrap_or_default();
    // a block body lowers to a `program` array; an expression body keeps a single `body` expr.
    let body = match object.get("program") {
        Some(program) => FunctionBody::Program(parse_program(program)?),
        None => {
            let body = object.get("body").ok_or_else(|| {
                WorkflowValidationError::InvalidValueRef("function requires a body".into())
            })?;
            FunctionBody::Expr(parse_expression(body)?)
        }
    };
    let max_depth = object
        .get("recursive")
        .and_then(Value::as_object)
        .and_then(|recursive| recursive.get("max_depth"))
        .and_then(Value::as_u64)
        .map(|depth| depth as u32);
    Ok(RuntimeFunction {
        params,
        body,
        max_depth,
    })
}

/// a parameter is either a bare name string or an object carrying at least a `name`.
fn param_name(value: &Value) -> Result<String, WorkflowValidationError> {
    if let Some(name) = value.as_str() {
        return Ok(name.to_string());
    }
    value
        .as_object()
        .and_then(|object| object.get("name"))
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| {
            WorkflowValidationError::InvalidValueRef("function parameter requires a name".into())
        })
}

mod runtime_function;
pub use runtime_function::RuntimeFunction;

mod function_table;
pub use function_table::FunctionTable;
