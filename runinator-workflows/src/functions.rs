// user-defined functions and the generated intrinsic catalog.
//
// the catalog is generated from the rust intrinsic metadata (`compute.rs`) so the wdl front end's
// view of the callable signatures cannot drift from what the runtime actually dispatches. user
// functions are carried in the workflow definition (`metadata.functions`) and evaluated by the
// expression engine: their bodies are hermetic single expressions over their parameters, applied by
// binding arguments into the `let` slot of a fresh scope. recursion is bounded by a per-function
// `max_depth` plus a global safety cap.

use std::collections::HashMap;

use runinator_models::providers::{ActionMetadata, ParameterMetadata, ResultMetadata};
use runinator_models::types::RuninatorType;
use runinator_models::value::{Map, Value};

use crate::compute::{IntrinsicLibrary, PureIntrinsics, effectful_signatures};
use crate::errors::WorkflowValidationError;
use crate::expressions::{evaluate_expression_with, parse_expression};
use crate::keys::REF_LOCAL;
use crate::types::WorkflowExpression;

/// a hard ceiling on nested user-function calls, independent of any per-function limit. guards
/// against runaway recursion that slipped past the front end's annotation checks.
pub(crate) const MAX_CALL_DEPTH: u32 = 1024;

/// every intrinsic's typed signature, generated from the rust metadata. the wdl front end consumes
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

/// a user-defined function resolved for runtime evaluation: parameter names (binding is positional),
/// the lowered body expression, and an optional recursion depth limit.
pub struct RuntimeFunction {
    pub params: Vec<String>,
    pub body: WorkflowExpression,
    pub max_depth: Option<u32>,
}

/// the user functions a workflow carries, keyed by name. parsed from `metadata.functions`.
#[derive(Default)]
pub struct FunctionTable {
    functions: HashMap<String, RuntimeFunction>,
}

impl FunctionTable {
    /// look up a function by name.
    pub fn get(&self, name: &str) -> Option<&RuntimeFunction> {
        self.functions.get(name)
    }

    /// whether the table carries no functions.
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty()
    }

    /// parse the `metadata.functions` array into a runtime table. `None` (no functions section)
    /// yields an empty table. each entry is `{ name, params: [{name,...}|"name"], body, recursive? }`.
    pub fn from_metadata(value: Option<&Value>) -> Result<Self, WorkflowValidationError> {
        let Some(value) = value else {
            return Ok(Self::default());
        };
        // a json `null` is the wire sentinel for "no functions section" (the std.exec dispatch
        // always carries a `functions` key), so treat it the same as an absent value.
        if value.is_null() {
            return Ok(Self::default());
        }
        let items = value.as_array().ok_or_else(|| {
            WorkflowValidationError::InvalidValueRef("metadata.functions must be an array".into())
        })?;
        let mut functions = HashMap::with_capacity(items.len());
        for item in items {
            let function = parse_function(item)?;
            let object = item.as_object();
            let name = object
                .and_then(|map| map.get("name"))
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    WorkflowValidationError::InvalidValueRef("function requires a name".into())
                })?;
            functions.insert(name.to_string(), function);
        }
        Ok(Self { functions })
    }
}

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
    let body = object.get("body").ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef("function requires a body".into())
    })?;
    let max_depth = object
        .get("recursive")
        .and_then(Value::as_object)
        .and_then(|recursive| recursive.get("max_depth"))
        .and_then(Value::as_u64)
        .map(|depth| depth as u32);
    Ok(RuntimeFunction {
        params,
        body: parse_expression(body)?,
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

/// the environment threaded through expression evaluation: the (optional) intrinsic library, the
/// (optional) user-function table, and the current user-call nesting depth. `Copy` so it passes
/// cheaply through the recursive evaluator; `deeper()` bumps the depth for a user-function body.
#[derive(Clone, Copy)]
pub(crate) struct EvalEnv<'a> {
    pub(crate) lib: Option<&'a dyn IntrinsicLibrary>,
    functions: Option<&'a FunctionTable>,
    depth: u32,
}

impl<'a> EvalEnv<'a> {
    /// an environment with a library and a function table.
    pub(crate) fn new(
        lib: Option<&'a dyn IntrinsicLibrary>,
        functions: Option<&'a FunctionTable>,
    ) -> Self {
        Self {
            lib,
            functions,
            depth: 0,
        }
    }

    /// an environment with a library but no user functions (declarative/preview paths).
    pub(crate) fn lib_only(lib: Option<&'a dyn IntrinsicLibrary>) -> Self {
        Self {
            lib,
            functions: None,
            depth: 0,
        }
    }

    /// resolve a user function by name, if a table is present.
    pub(crate) fn lookup(&self, name: &str) -> Option<&'a RuntimeFunction> {
        self.functions.and_then(|table| table.get(name))
    }

    /// the same environment one user-call deeper.
    fn deeper(self) -> Self {
        Self {
            depth: self.depth + 1,
            ..self
        }
    }
}

/// invoke a user function: bind `values` to its parameters in a fresh hermetic scope (only the
/// params are visible) and evaluate its body, enforcing the recursion limits.
pub(crate) fn invoke_user_function(
    name: &str,
    function: &RuntimeFunction,
    values: &[Value],
    env: EvalEnv,
) -> Result<Value, WorkflowValidationError> {
    if env.depth >= MAX_CALL_DEPTH {
        return Err(WorkflowValidationError::InvalidValueRef(format!(
            "maximum call depth exceeded calling '{name}'"
        )));
    }
    if let Some(max) = function.max_depth
        && env.depth >= max
    {
        return Err(WorkflowValidationError::InvalidValueRef(format!(
            "recursion limit ({max}) exceeded for '{name}'"
        )));
    }
    let mut locals = Map::new();
    for (param, value) in function.params.iter().zip(values.iter()) {
        locals.insert(param.clone(), value.clone());
    }
    let scope = Value::Object(Map::from_iter([(REF_LOCAL.into(), Value::Object(locals))]));
    evaluate_expression_with(&function.body, &scope, env.deeper())
}
