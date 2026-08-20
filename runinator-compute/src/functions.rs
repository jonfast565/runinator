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

use crate::assemble::assemble_module;
use crate::catalog::CallableCatalog;
use crate::compute::{IntrinsicLibrary, PureIntrinsics, effectful_signatures, parse_program};
use crate::errors::WorkflowValidationError;
use crate::expressions::parse_expression;
use crate::vm::{VmEnv, start};
use runinator_models::invocation::{
    CallableTarget, InvocationInstruction, InvocationModule, InvocationProgram, InvocationStep,
};
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
pub struct RuntimeFunction {
    pub params: Vec<String>,
    pub body: FunctionBody,
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

/// the environment threaded through expression evaluation.
#[derive(Clone, Copy)]
pub(crate) struct EvalEnv<'a> {
    pub(crate) lib: Option<&'a dyn IntrinsicLibrary>,
    functions: Option<&'a FunctionTable>,
}

impl<'a> EvalEnv<'a> {
    /// an environment with a library and a function table.
    pub(crate) fn new(
        lib: Option<&'a dyn IntrinsicLibrary>,
        functions: Option<&'a FunctionTable>,
    ) -> Self {
        Self { lib, functions }
    }

    /// an environment with a library but no user functions (declarative/preview paths).
    pub(crate) fn lib_only(lib: Option<&'a dyn IntrinsicLibrary>) -> Self {
        Self {
            lib,
            functions: None,
        }
    }

    /// resolve a user function by name, if a table is present.
    pub(crate) fn lookup(&self, name: &str) -> Option<&'a RuntimeFunction> {
        self.functions.and_then(|table| table.get(name))
    }
}

/// invoke a user function: bind `values` to its parameters in a fresh hermetic scope (only the
/// params are visible) and evaluate its body, enforcing the recursion limits.
pub(crate) fn invoke_user_function(
    name: &str,
    _function: &RuntimeFunction,
    values: &[Value],
    env: EvalEnv,
) -> Result<Value, WorkflowValidationError> {
    let table = env.functions.ok_or_else(|| {
        WorkflowValidationError::InvalidValueRef(format!("unknown function '{name}'"))
    })?;
    let module = table.module_for_call(name, values)?;
    let catalog = table.catalog();
    match start(&module, &VmEnv::pure(&Value::Null, &catalog)) {
        InvocationStep::Complete { value } => Ok(value),
        InvocationStep::Goto { .. } => Err(WorkflowValidationError::InvalidValueRef(format!(
            "goto is not allowed in function '{name}'"
        ))),
        InvocationStep::Yield { effect, .. } => {
            Err(WorkflowValidationError::InvalidValueRef(format!(
                "'{}' cannot be called in a declarative function",
                effect.target.display_name()
            )))
        }
        InvocationStep::Failed { message } => {
            Err(WorkflowValidationError::InvalidValueRef(message))
        }
    }
}

impl FunctionTable {
    fn catalog(&self) -> CallableCatalog {
        let mut catalog = CallableCatalog::builtin();
        for (name, function) in &self.functions {
            catalog.add_local(
                name.clone(),
                function.params.len(),
                runinator_models::invocation::EffectClass::Pure,
            );
        }
        catalog
    }

    fn module_for_call(
        &self,
        name: &str,
        values: &[Value],
    ) -> Result<InvocationModule, WorkflowValidationError> {
        let functions =
            self.functions
                .iter()
                .map(|(name, function)| {
                    let body = match &function.body {
                        FunctionBody::Expr(expression) => ComputeProgram(vec![
                            runinator_models::workflow_ast::ComputeStmt::Return(expression.clone()),
                        ]),
                        FunctionBody::Program(program) => program.clone(),
                    };
                    (
                        name.clone(),
                        function.params.clone(),
                        body,
                        function.max_depth,
                    )
                })
                .collect::<Vec<_>>();
        let entry = InvocationProgram::new(
            values
                .iter()
                .cloned()
                .map(|value| InvocationInstruction::Const { value })
                .chain([
                    InvocationInstruction::Call {
                        target: CallableTarget::Local {
                            name: name.to_string(),
                        },
                        argc: values.len(),
                        names: Vec::new(),
                        policy: None,
                    },
                    InvocationInstruction::Return,
                ])
                .collect(),
        );
        let module = assemble_module(&ComputeProgram::default(), &functions, &self.catalog())?;
        Ok(InvocationModule { entry, ..module })
    }
}
