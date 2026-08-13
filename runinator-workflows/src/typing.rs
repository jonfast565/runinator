use std::collections::HashMap;

use runinator_models::value::Value;
use runinator_models::{
    providers::{ParameterMetadata, ProviderMetadata, validate_provider_metadata},
    types::{RuninatorField, RuninatorType, TypeViolation},
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowWaitSeconds},
};

use runinator_compute::keys::{
    COND_ALL, COND_ANY, COND_CONTAINS, COND_ENDS_WITH, COND_EQUALS, COND_EXISTS, COND_GREATER_THAN,
    COND_GREATER_THAN_OR_EQUAL, COND_IN, COND_LEFT, COND_LESS_THAN, COND_LESS_THAN_OR_EQUAL,
    COND_NOT, COND_NOT_EQUALS, COND_STARTS_WITH, COND_VALUE,
};
use runinator_compute::{
    WorkflowTypeDiagnostic, WorkflowValidationError, parse_expression, serialize_value_ref,
    validate_condition,
};

use crate::{
    node_kinds::{ActionCatalog, spec_for},
    parameters::{
        parse_join_parameters, parse_map_parameters, parse_parallel_parameters,
        parse_percentage_parameters, parse_race_parameters, parse_switch_parameters,
        parse_toggle_parameters, parse_try_parameters,
    },
};
use runinator_models::workflow_ast::{
    WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef,
};

pub type WorkflowType = RuninatorType;

#[derive(Debug, Clone)]
struct TypeContext {
    input: WorkflowType,
    workflow: WorkflowType,
    config: WorkflowType,
    node_outputs: HashMap<String, WorkflowType>,
    locals: Vec<(String, WorkflowType)>,
}

pub fn validate_workflow_types(
    workflow: &WorkflowDefinition,
    nodes: &[WorkflowNode],
    providers: &[ProviderMetadata],
    config_type: &WorkflowType,
) -> Result<(), WorkflowValidationError> {
    let provider_actions = ActionCatalog::new(providers);
    validate_provider_metadata_set(providers)?;
    let mut context = TypeContext {
        input: workflow.input_type.clone(),
        workflow: workflow_context_type(),
        config: config_type.clone(),
        node_outputs: HashMap::new(),
        locals: Vec::new(),
    };

    for node in nodes {
        if let Some(output_type) = spec_for(&node.kind).output_type(node, &provider_actions)? {
            context
                .node_outputs
                .insert(node.id.as_str().to_string(), output_type);
        }
    }
    for (node_id, output_type) in declared_node_output_types(workflow)? {
        context.node_outputs.insert(node_id, output_type);
    }
    for node in nodes {
        let output_type = match node.kind {
            WorkflowNodeKind::Loop => context.loop_node_output_type(node)?,
            WorkflowNodeKind::Map => context.map_node_output_type(node)?,
            _ => None,
        };
        if let Some(output_type) = output_type {
            context
                .node_outputs
                .insert(node.id.as_str().to_string(), output_type);
        }
    }

    for node in nodes {
        context.validate_condition_types(&node.condition.to_value())?;
        for branch in &node.transitions.branches {
            context.validate_condition_types(&branch.when.to_value())?;
        }
        context.validate_node_types(node, &provider_actions)?;
    }

    Ok(())
}

fn declared_node_output_types(
    workflow: &WorkflowDefinition,
) -> Result<HashMap<String, WorkflowType>, WorkflowValidationError> {
    let Some(entries) = workflow
        .definition
        .metadata
        .pointer("/wdl/type_hints")
        .and_then(Value::as_object)
    else {
        return Ok(HashMap::new());
    };

    let mut types = HashMap::new();
    for (node_id, value) in entries {
        let ty = value.decode::<WorkflowType>().map_err(|err| {
            WorkflowValidationError::TypeError(format!(
                "workflow metadata.wdl.type_hints['{}'] is invalid: {}",
                node_id, err
            ))
        })?;
        types.insert(node_id.clone(), ty);
    }
    Ok(types)
}

fn validate_provider_metadata_set(
    providers: &[ProviderMetadata],
) -> Result<(), WorkflowValidationError> {
    for provider in providers {
        validate_provider_metadata(provider).map_err(WorkflowValidationError::TypeError)?;
    }
    Ok(())
}

impl TypeContext {
    /// `steps.<loop>.output`: the whole `LoopOutput` the reducer emits, not just the item binding.
    ///
    /// declaring `{item, index}` while the runtime emitted five fields meant `count`, `has_next`,
    /// and `last` were written on every visit and rejected as `MissingRef` by every reader — the
    /// three fields that answer "where am I" were the three that could not be read.
    fn loop_node_output_type(
        &self,
        node: &WorkflowNode,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        // loop items are resolved data (a raw `Value`), unlike map's typed expression.
        let Some(items) = node.parameters.get("items") else {
            return Ok(None);
        };
        let item_type = collection_item_type("loop.items", &self.infer_value_type(items)?)?;
        Ok(Some(WorkflowType::typed_structure([
            // absent on the exhausting visit — the visit that takes the exit edge. this is an
            // optional *field* rather than a `union<T, null>` because `resolve_path_type` requires
            // every union variant to resolve the rest of a path, so a null variant would turn
            // every `steps.<loop>.output.item.field` in a loop body into a `MissingRef`.
            ("item", RuninatorField::optional(item_type)),
            ("index", RuninatorField::required(WorkflowType::Integer)),
            ("has_next", RuninatorField::required(WorkflowType::Boolean)),
            ("count", RuninatorField::required(WorkflowType::Integer)),
            // the previous lap's body output; no statically-known shape.
            ("last", RuninatorField::optional(WorkflowType::Any)),
            (
                "results",
                RuninatorField::required(WorkflowType::array(WorkflowType::Any)),
            ),
        ])))
    }

    /// `steps.<map>.output`, which names two different runtime values under one node id: the child
    /// run sees the `{item, index}` binding the body's loop variable reads, and everything
    /// downstream sees `MapOutput { count, outputs }` once the fan-out settles. one entry has to
    /// serve both, so every field is optional and the struct admits the union.
    fn map_node_output_type(
        &self,
        node: &WorkflowNode,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        let items_type = self.infer_expression_type(&parse_map_parameters(node)?.items)?;
        let item_type = collection_item_type("map.items", &items_type)?;
        Ok(Some(WorkflowType::typed_structure([
            ("item", RuninatorField::optional(item_type)),
            ("index", RuninatorField::optional(WorkflowType::Integer)),
            ("count", RuninatorField::optional(WorkflowType::Integer)),
            // per-item body results, whose shape is not statically known.
            (
                "outputs",
                RuninatorField::optional(WorkflowType::array(WorkflowType::Any)),
            ),
        ])))
    }

    fn validate_node_types(
        &self,
        node: &WorkflowNode,
        provider_actions: &ActionCatalog<'_>,
    ) -> Result<(), WorkflowValidationError> {
        match node.kind {
            WorkflowNodeKind::Action => self.validate_action_configuration(node, provider_actions),
            WorkflowNodeKind::Wait => {
                if let Some(seconds) = node.wait.seconds.as_ref() {
                    match seconds {
                        WorkflowWaitSeconds::Integer(value) if *value < 0 => {
                            return Err(WorkflowValidationError::TypeError(format!(
                                "node '{}' wait.seconds must be greater than or equal to zero",
                                node.id
                            )));
                        }
                        WorkflowWaitSeconds::Expression(expression) => {
                            self.expect_value_type(
                                expression.as_value(),
                                &WorkflowType::Integer,
                                "wait.seconds",
                            )?;
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            WorkflowNodeKind::Switch => {
                let params = parse_switch_parameters(node)?;
                self.infer_expression_type(&params.value)?;
                for case in params.cases {
                    self.validate_condition_types(&case.condition.to_value())?;
                }
                Ok(())
            }
            WorkflowNodeKind::Toggle => {
                let params = parse_toggle_parameters(node)?;
                self.infer_expression_type(&params.value)?;
                Ok(())
            }
            WorkflowNodeKind::Percentage => {
                let params = parse_percentage_parameters(node)?;
                self.infer_expression_type(&params.key)?;
                Ok(())
            }
            // both iterables go through `collection_item_type`, which tolerates `any` and a union
            // of arrays and rejects everything else. a bare `matches!(Array(_))` rejected `any`
            // too, so a loop over an untyped upstream output — the common case — passed
            // `runinator-wdl-sema`'s `check_iterable` and then failed here at import.
            WorkflowNodeKind::Loop => {
                let Some(items) = node.parameters.get("items") else {
                    return Err(WorkflowValidationError::InvalidNodeParameters {
                        node: node.id.as_str().to_string(),
                        message: "loop.items is required".into(),
                    });
                };
                collection_item_type("loop.items", &self.infer_value_type(items)?)?;
                Ok(())
            }
            WorkflowNodeKind::Map => {
                let params = parse_map_parameters(node)?;
                collection_item_type("map.items", &self.infer_expression_type(&params.items)?)?;
                Ok(())
            }
            WorkflowNodeKind::Parallel => {
                parse_parallel_parameters(node)?;
                Ok(())
            }
            WorkflowNodeKind::Join => {
                parse_join_parameters(node)?;
                Ok(())
            }
            WorkflowNodeKind::Try => {
                parse_try_parameters(node)?;
                Ok(())
            }
            WorkflowNodeKind::Race => {
                parse_race_parameters(node)?;
                Ok(())
            }
            WorkflowNodeKind::Condition
            | WorkflowNodeKind::Start
            | WorkflowNodeKind::End
            | WorkflowNodeKind::Fail => Ok(()),
            WorkflowNodeKind::Subflow => {
                if let Some(run_name) = node.subflow.run_name.as_ref() {
                    self.expect_value_type(run_name, &WorkflowType::String, "subflow.run_name")?;
                }
                self.infer_value_type(&node.parameters)?;
                Ok(())
            }
            WorkflowNodeKind::Config => {
                if let Some(name) = node.parameters.get("name") {
                    self.expect_value_type(name, &WorkflowType::String, "config.name")?;
                }
                if let Some(metadata) = node.parameters.get("metadata") {
                    self.infer_value_type(metadata)?;
                }
                Ok(())
            }
            WorkflowNodeKind::Approval => {
                if let Some(approval_type) = node.parameters.get("approval_type") {
                    self.expect_value_type(
                        approval_type,
                        &WorkflowType::String,
                        "approval.approval_type",
                    )?;
                }
                if let Some(prompt) = node.parameters.get("prompt") {
                    self.expect_value_type(prompt, &WorkflowType::String, "approval.prompt")?;
                }
                Ok(())
            }
            WorkflowNodeKind::Output => {
                if let Some(event_type) = node.parameters.get("event_type") {
                    self.expect_value_type(event_type, &WorkflowType::String, "output.event_type")?;
                }
                if let Some(data) = node.parameters.get("data") {
                    self.infer_value_type(data)?;
                }
                Ok(())
            }
            WorkflowNodeKind::Input => {
                if let Some(prompt) = node.parameters.get("prompt") {
                    self.expect_value_type(prompt, &WorkflowType::String, "input.prompt")?;
                }
                Ok(())
            }
            WorkflowNodeKind::Gate => {
                // condition gates carry a `when` condition the reducer auto-evaluates; type-check it.
                if let Some(when) = node.parameters.get("when") {
                    self.validate_condition_types(when)?;
                }
                Ok(())
            }
            WorkflowNodeKind::Signal => {
                if let Some(name) = node.parameters.get("name") {
                    self.expect_value_type(name, &WorkflowType::String, "signal.name")?;
                }
                Ok(())
            }
            _ => {
                self.infer_value_type(&node.parameters)?;
                Ok(())
            }
        }
    }

    fn validate_action_configuration(
        &self,
        node: &WorkflowNode,
        provider_actions: &ActionCatalog<'_>,
    ) -> Result<(), WorkflowValidationError> {
        let action = node
            .action
            .as_ref()
            .ok_or_else(|| WorkflowValidationError::MissingAction(node.id.as_str().to_string()))?;
        let metadata = provider_actions
            .get(&action.provider, &action.function)
            .ok_or_else(|| {
                WorkflowValidationError::TypeError(format!(
                    "node '{}' references unknown provider action '{}.{}'",
                    node.id, action.provider, action.function
                ))
            })?;
        let Some(configuration) = action.configuration.as_object() else {
            return Err(WorkflowValidationError::InvalidNodeParameters {
                node: node.id.as_str().to_string(),
                message: "action.configuration must be an object".into(),
            });
        };
        let params = metadata
            .parameters
            .iter()
            .map(|param| (param.name.as_str(), param))
            .collect::<HashMap<_, _>>();

        for param in &metadata.parameters {
            if param.required
                && configuration
                    .get(&param.name)
                    .is_none_or(is_blank_parameter_value)
            {
                return Err(WorkflowValidationError::TypeError(format!(
                    "node '{}' is missing required action parameter '{}'",
                    node.id, param.name
                )));
            }
        }
        for (name, value) in configuration {
            let Some(param) = params.get(name.as_str()) else {
                return Err(WorkflowValidationError::TypeError(format!(
                    "node '{}' has unknown action parameter '{}'",
                    node.id, name
                )));
            };
            self.expect_parameter_value_type(value, &parameter_type(param), name)?;
        }
        Ok(())
    }

    fn infer_value_type(&self, value: &Value) -> Result<WorkflowType, WorkflowValidationError> {
        self.infer_expression_type(&parse_expression(value)?)
    }

    fn infer_expression_type(
        &self,
        expression: &WorkflowExpression,
    ) -> Result<WorkflowType, WorkflowValidationError> {
        match expression {
            WorkflowExpression::Literal(value) => self.literal_type(value),
            WorkflowExpression::Ref(reference) => self.resolve_ref_type(reference),
            WorkflowExpression::Concat(items) => {
                for item in items {
                    let ty = self.infer_expression_type(item)?;
                    if ty != WorkflowType::String {
                        return Err(WorkflowValidationError::TypeError(format!(
                            "$concat item must be string, got {}",
                            ty.describe()
                        )));
                    }
                }
                Ok(WorkflowType::String)
            }
            WorkflowExpression::Coalesce(items) => {
                let mut resolved: Option<WorkflowType> = None;
                for item in items {
                    let ty = self.infer_expression_type(item)?;
                    if ty == WorkflowType::Null {
                        continue;
                    }
                    resolved = Some(match resolved {
                        None => ty,
                        Some(existing) => existing.unify(&ty),
                    });
                }
                Ok(resolved.unwrap_or(WorkflowType::Null))
            }
            WorkflowExpression::ToString(nested) => {
                let ty = self.infer_expression_type(nested)?;
                if ty.is_primitive() || matches!(ty, WorkflowType::Any | WorkflowType::Union(_)) {
                    Ok(WorkflowType::String)
                } else {
                    Err(WorkflowValidationError::TypeError(format!(
                        "$to_string requires a primitive value, got {}",
                        ty.describe()
                    )))
                }
            }
            WorkflowExpression::ToJsonString(nested) => {
                let ty = self.infer_expression_type(nested)?;
                if matches!(
                    ty,
                    WorkflowType::Array(_)
                        | WorkflowType::Map(_)
                        | WorkflowType::Struct { .. }
                        | WorkflowType::Any
                ) {
                    Ok(WorkflowType::String)
                } else {
                    Err(WorkflowValidationError::TypeError(format!(
                        "$to_json_string requires an array, map, struct, or any value, got {}",
                        ty.describe()
                    )))
                }
            }
            // arithmetic resolves to a numeric type; require every operand to be numeric.
            WorkflowExpression::Add(items)
            | WorkflowExpression::Sub(items)
            | WorkflowExpression::Mul(items)
            | WorkflowExpression::Div(items)
            | WorkflowExpression::Mod(items) => {
                let mut all_integer = true;
                for item in items {
                    let ty = self.infer_expression_type(item)?;
                    match ty {
                        WorkflowType::Integer => {}
                        WorkflowType::Number | WorkflowType::Any => all_integer = false,
                        other => {
                            return Err(WorkflowValidationError::TypeError(format!(
                                "arithmetic operand must be numeric, got {}",
                                other.describe()
                            )));
                        }
                    }
                }
                Ok(if all_integer {
                    WorkflowType::Integer
                } else {
                    WorkflowType::Number
                })
            }
            WorkflowExpression::Neg(nested) => {
                let ty = self.infer_expression_type(nested)?;
                match ty {
                    WorkflowType::Integer => Ok(WorkflowType::Integer),
                    WorkflowType::Number | WorkflowType::Any => Ok(WorkflowType::Number),
                    other => Err(WorkflowValidationError::TypeError(format!(
                        "arithmetic operand must be numeric, got {}",
                        other.describe()
                    ))),
                }
            }
            WorkflowExpression::Call { name, args } => {
                if crate::is_higher_order(name) {
                    return self.infer_higher_order_type(name, args);
                }
                // a call to a local bound to a first-class lambda yields the function's result type.
                if let Some(WorkflowType::Function { ret, .. }) = self.function_local(name) {
                    return Ok((**ret).clone());
                }
                // recover an argument-dependent result type for the polymorphic intrinsics before
                // falling back to the catalog's declared (often `any`) result type.
                let arg_types = args
                    .iter()
                    .map(|arg| self.infer_expression_type(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                // extract any literal key(s) from the second argument for key-driven intrinsics.
                let literal_keys = args.get(1).and_then(static_string_keys);
                if let Some(ty) =
                    crate::intrinsic_result_type(name, &arg_types, literal_keys.as_deref())
                {
                    return Ok(ty);
                }
                Ok(crate::intrinsic_signature(name)
                    .and_then(|signature| signature.results.first().map(|result| result.ty.clone()))
                    .unwrap_or(WorkflowType::Any))
            }
            // a lambda's value type: unconstrained parameters and the body's result type.
            WorkflowExpression::Lambda { params, body } => {
                let mut scoped = self.clone();
                for param in params {
                    scoped.locals.push((param.clone(), WorkflowType::Any));
                }
                let ret = scoped.infer_expression_type(body)?;
                Ok(WorkflowType::Function {
                    params: params.iter().map(|_| WorkflowType::Any).collect(),
                    ret: Box::new(ret),
                })
            }
            // a conditional resolves to the common type of its branches (the condition is not typed here).
            WorkflowExpression::Cond {
                then, otherwise, ..
            } => {
                let then_ty = self.infer_expression_type(then)?;
                let otherwise_ty = self.infer_expression_type(otherwise)?;
                Ok(then_ty.unify(&otherwise_ty))
            }
            // applying a callee value: the callee must be a function; yield its declared result type,
            // checking arity. an opaque (`any`) callee stays permissive.
            WorkflowExpression::Apply { callee, args } => {
                let callee_type = self.infer_expression_type(callee)?;
                for arg in args {
                    self.infer_expression_type(arg)?;
                }
                match callee_type {
                    WorkflowType::Function { params, ret } => {
                        if params.len() != args.len() {
                            return Err(WorkflowValidationError::TypeError(format!(
                                "applied function expects {} argument(s), got {}",
                                params.len(),
                                args.len()
                            )));
                        }
                        Ok(*ret)
                    }
                    WorkflowType::Any => Ok(WorkflowType::Any),
                    other => Err(WorkflowValidationError::TypeError(format!(
                        "cannot apply a value of type {}",
                        other.describe()
                    ))),
                }
            }
        }
    }

    fn infer_higher_order_type(
        &self,
        name: &str,
        args: &[WorkflowExpression],
    ) -> Result<WorkflowType, WorkflowValidationError> {
        let arg = |index: usize| {
            args.get(index).ok_or_else(|| {
                WorkflowValidationError::TypeError(format!("'{name}' is missing an argument"))
            })
        };
        let collection_type = self.infer_expression_type(arg(0)?)?;
        let item_type = collection_item_type(name, &collection_type)?;
        match name {
            "map" => {
                let body_type = self.infer_lambda_type(name, arg(1)?, &[(0, item_type)])?;
                Ok(WorkflowType::array(body_type))
            }
            "flat_map" => {
                let body_type = self.infer_lambda_type(name, arg(1)?, &[(0, item_type)])?;
                Ok(match body_type {
                    WorkflowType::Array(inner) => WorkflowType::array(*inner),
                    other => WorkflowType::array(other),
                })
            }
            "filter" | "sort_by" => {
                let body_type = self.infer_lambda_type(name, arg(1)?, &[(0, item_type.clone())])?;
                if name == "filter" {
                    expect_type(&body_type, &WorkflowType::Boolean, "'filter' lambda")?;
                }
                Ok(WorkflowType::array(item_type))
            }
            "find" => {
                let body_type = self.infer_lambda_type(name, arg(1)?, &[(0, item_type.clone())])?;
                expect_type(&body_type, &WorkflowType::Boolean, "'find' lambda")?;
                Ok(WorkflowType::Union(vec![item_type, WorkflowType::Null]))
            }
            "any" | "all" => {
                let body_type = self.infer_lambda_type(name, arg(1)?, &[(0, item_type)])?;
                expect_type(
                    &body_type,
                    &WorkflowType::Boolean,
                    &format!("'{name}' lambda"),
                )?;
                Ok(WorkflowType::Boolean)
            }
            "reduce" => {
                let accumulator_type = self.infer_expression_type(arg(1)?)?;
                let body_type = self.infer_lambda_type(
                    name,
                    arg(2)?,
                    &[(0, accumulator_type.clone()), (1, item_type)],
                )?;
                if let Some(result_type) = common_type(accumulator_type.clone(), body_type.clone())
                {
                    return Ok(result_type);
                }
                expect_type(&body_type, &accumulator_type, "'reduce' lambda")?;
                Ok(accumulator_type)
            }
            _ => Ok(WorkflowType::Any),
        }
    }

    fn infer_lambda_type(
        &self,
        name: &str,
        expression: &WorkflowExpression,
        bindings: &[(usize, WorkflowType)],
    ) -> Result<WorkflowType, WorkflowValidationError> {
        let WorkflowExpression::Lambda { params, body } = expression else {
            // a first-class function value passed by reference: use its result type when the arity
            // matches, and stay permissive for an opaque (`any`) reference.
            return match self.infer_expression_type(expression)? {
                WorkflowType::Function {
                    params: fn_params,
                    ret,
                } if fn_params.len() == bindings.len() => Ok(*ret),
                WorkflowType::Any => Ok(WorkflowType::Any),
                _ => Err(WorkflowValidationError::TypeError(format!(
                    "'{name}' requires a lambda argument"
                ))),
            };
        };
        let required = bindings.len();
        if params.len() != required {
            return Err(WorkflowValidationError::TypeError(format!(
                "'{name}' lambda expects {required} parameter(s), got {}",
                params.len()
            )));
        }
        let mut scoped = self.clone();
        for (index, ty) in bindings {
            scoped.locals.push((params[*index].clone(), ty.clone()));
        }
        scoped.infer_expression_type(body)
    }

    fn literal_type(&self, value: &Value) -> Result<WorkflowType, WorkflowValidationError> {
        match value {
            Value::Null => Ok(WorkflowType::Null),
            Value::Bool(_) => Ok(WorkflowType::Boolean),
            Value::Number(number) if number.is_i64() || number.is_u64() => {
                Ok(WorkflowType::Integer)
            }
            Value::Number(_) => Ok(WorkflowType::Number),
            Value::String(_) => Ok(WorkflowType::String),
            Value::Array(items) => {
                let mut item_type = None;
                for item in items {
                    let ty = self.infer_value_type(item)?;
                    item_type = Some(match item_type {
                        None => ty,
                        Some(existing) => common_type(existing, ty).ok_or_else(|| {
                            WorkflowValidationError::TypeError(
                                "array literal contains incompatible item types".into(),
                            )
                        })?,
                    });
                }
                Ok(WorkflowType::Array(Box::new(
                    item_type.unwrap_or(WorkflowType::Any),
                )))
            }
            Value::Object(fields) => Ok(WorkflowType::structure(
                fields
                    .iter()
                    .map(|(key, value)| self.infer_value_type(value).map(|ty| (key.clone(), ty)))
                    .collect::<Result<Vec<_>, _>>()?,
            )),
        }
    }

    fn resolve_ref_type(
        &self,
        reference: &WorkflowValueRef,
    ) -> Result<WorkflowType, WorkflowValidationError> {
        if matches!(&reference.source, WorkflowRefSource::Local) {
            let Some(WorkflowPathSegment::Key(head)) = reference.path.first() else {
                return Ok(WorkflowType::Any);
            };
            let Some((_, ty)) = self.locals.iter().rev().find(|(name, _)| name == head) else {
                return Ok(WorkflowType::Any);
            };
            return resolve_path_type(ty, &reference.path[1..]).ok_or_else(|| {
                WorkflowValidationError::MissingRef(serialize_value_ref(reference).to_string())
            });
        }
        let base = match &reference.source {
            WorkflowRefSource::Input => &self.input,
            WorkflowRefSource::Workflow => &self.workflow,
            WorkflowRefSource::Prev => &WorkflowType::Any,
            // config is typed from the stored settings schema (`{ scope: { name: type } }`); an
            // open struct keeps not-yet-configured keys permissive (`any`) instead of erroring.
            WorkflowRefSource::Config => &self.config,
            // what an interrupt carries depends on the source that raised it, so the payload has no
            // statically-known shape; stay permissive rather than inventing one per source.
            WorkflowRefSource::Interrupt => return Ok(WorkflowType::Any),
            // local refs are fully resolved by the block above; if one ever reaches here, stay
            // permissive (`any`) like the local fallback rather than panicking.
            WorkflowRefSource::Local => return Ok(WorkflowType::Any),
            WorkflowRefSource::NodeOutput(node) => {
                self.node_outputs.get(node.as_str()).ok_or_else(|| {
                    WorkflowValidationError::MissingRef(serialize_value_ref(reference).to_string())
                })?
            }
        };
        resolve_path_type(base, &reference.path).ok_or_else(|| {
            WorkflowValidationError::MissingRef(serialize_value_ref(reference).to_string())
        })
    }
}

fn parameter_type(param: &ParameterMetadata) -> WorkflowType {
    param.ty.clone()
}

fn collection_item_type(
    name: &str,
    ty: &WorkflowType,
) -> Result<WorkflowType, WorkflowValidationError> {
    match ty {
        WorkflowType::Array(item) => Ok((**item).clone()),
        // a union of iterables keeps its unioned element type; anything else opaque stays `any`.
        WorkflowType::Union(_) => Ok(ty.union_element_type().unwrap_or(WorkflowType::Any)),
        WorkflowType::Any => Ok(WorkflowType::Any),
        other => Err(WorkflowValidationError::TypeError(format!(
            "'{name}' requires an array, got {}",
            other.describe()
        ))),
    }
}

fn resolve_path_type(base: &WorkflowType, path: &[WorkflowPathSegment]) -> Option<WorkflowType> {
    let mut current = base.clone();
    for (index, segment) in path.iter().enumerate() {
        current = match (segment, &current) {
            // an `any` base absorbs any path: drilling into the unknown stays unknown.
            (_, WorkflowType::Any) => return Some(WorkflowType::Any),
            // a union resolves the rest of the path on each variant and re-unions the results, so a
            // field common to every variant keeps a concrete type instead of collapsing to `any`.
            (_, WorkflowType::Union(variants)) => {
                let rest = &path[index..];
                let mut resolved: Option<WorkflowType> = None;
                for variant in variants {
                    let ty = resolve_path_type(variant, rest)?;
                    resolved = Some(match resolved {
                        None => ty,
                        Some(existing) => existing.unify(&ty),
                    });
                }
                return resolved;
            }
            (WorkflowPathSegment::Key(key), WorkflowType::Struct { .. } | WorkflowType::Map(_)) => {
                current.field(key)?.clone()
            }
            (WorkflowPathSegment::Index(_), WorkflowType::Array(item)) => (**item).clone(),
            _ => return None,
        };
    }
    Some(current)
}

impl TypeContext {
    fn validate_condition_types(&self, condition: &Value) -> Result<(), WorkflowValidationError> {
        validate_condition(condition)?;
        if condition.is_null() {
            return Ok(());
        }
        let object = condition.as_object().ok_or_else(|| {
            WorkflowValidationError::InvalidCondition("condition must be an object".into())
        })?;
        if let Some(all) = object.get(COND_ALL) {
            let Some(items) = all.as_array() else {
                return Err(WorkflowValidationError::InvalidCondition(
                    "all must be an array".into(),
                ));
            };
            for item in items {
                self.validate_condition_types(item)?;
            }
            return Ok(());
        }
        if let Some(any) = object.get(COND_ANY) {
            let Some(items) = any.as_array() else {
                return Err(WorkflowValidationError::InvalidCondition(
                    "any must be an array".into(),
                ));
            };
            for item in items {
                self.validate_condition_types(item)?;
            }
            return Ok(());
        }
        if let Some(not) = object.get(COND_NOT) {
            self.validate_condition_types(not)?;
            return Ok(());
        }

        let left = object
            .get(COND_VALUE)
            .or_else(|| object.get(COND_LEFT))
            .ok_or_else(|| WorkflowValidationError::InvalidCondition("missing value".into()))?;
        let left_type = self.infer_value_type(left)?;
        if let Some(expected) = object.get(COND_EQUALS) {
            comparable_types(&left_type, &self.infer_value_type(expected)?)?;
            return Ok(());
        }
        if let Some(expected) = object.get(COND_NOT_EQUALS) {
            comparable_types(&left_type, &self.infer_value_type(expected)?)?;
            return Ok(());
        }
        if let Some(expected) = object.get(COND_CONTAINS) {
            let expected_type = self.infer_value_type(expected)?;
            return validate_contains_type(&left_type, &expected_type);
        }
        if let Some(expected) = object.get(COND_IN) {
            let expected_type = self.infer_value_type(expected)?;
            let WorkflowType::Array(item_type) = expected_type else {
                return Err(WorkflowValidationError::TypeError(
                    "condition 'in' requires an array operand".into(),
                ));
            };
            assignable_type(&left_type, &item_type)?;
            return Ok(());
        }
        if let Some(expected) = object
            .get(COND_STARTS_WITH)
            .or_else(|| object.get(COND_ENDS_WITH))
        {
            expect_type(&left_type, &WorkflowType::String, "string condition value")?;
            expect_type(
                &self.infer_value_type(expected)?,
                &WorkflowType::String,
                "string condition operand",
            )?;
            return Ok(());
        }
        if let Some(expected) = object
            .get(COND_GREATER_THAN)
            .or_else(|| object.get(COND_GREATER_THAN_OR_EQUAL))
            .or_else(|| object.get(COND_LESS_THAN))
            .or_else(|| object.get(COND_LESS_THAN_OR_EQUAL))
        {
            let right_type = self.infer_value_type(expected)?;
            if (left_type.is_numeric() && right_type.is_numeric())
                || (left_type == WorkflowType::String && right_type == WorkflowType::String)
            {
                return Ok(());
            }
            return Err(WorkflowValidationError::TypeError(
                "ordering comparison requires both values to be numbers or strings".into(),
            ));
        }
        if let Some(expected) = object.get(COND_EXISTS) {
            self.expect_value_type(expected, &WorkflowType::Boolean, "exists")?;
            return Ok(());
        }
        if object.len() == 1 && object.contains_key(COND_VALUE) {
            return Ok(());
        }
        Ok(())
    }
}

fn validate_contains_type(
    left: &WorkflowType,
    expected: &WorkflowType,
) -> Result<(), WorkflowValidationError> {
    match left {
        WorkflowType::String => expect_type(expected, &WorkflowType::String, "contains operand"),
        WorkflowType::Array(item_type) => assignable_type(expected, item_type),
        WorkflowType::Map(_) | WorkflowType::Struct { .. } => {
            expect_type(expected, &WorkflowType::String, "object key")
        }
        _ => Err(WorkflowValidationError::TypeError(
            "contains requires a string, array, map, or struct value".into(),
        )),
    }
}

impl TypeContext {
    fn expect_value_type(
        &self,
        value: &Value,
        expected: &WorkflowType,
        label: &str,
    ) -> Result<(), WorkflowValidationError> {
        let actual = self.infer_value_type(value)?;
        expect_type(&actual, expected, label)
    }

    fn expect_parameter_value_type(
        &self,
        value: &Value,
        expected: &WorkflowType,
        name: &str,
    ) -> Result<(), WorkflowValidationError> {
        let label = format!("action parameter '{name}'");
        self.expect_mixed_value_type(value, expected, &label)
    }

    fn expect_mixed_value_type(
        &self,
        value: &Value,
        expected: &WorkflowType,
        label: &str,
    ) -> Result<(), WorkflowValidationError> {
        if is_expression_object(value) {
            let expression = parse_expression(value)?;
            if let WorkflowExpression::Literal(literal) = &expression {
                return expected
                    .validate_value(literal)
                    .map_err(|violation| type_error(label, &violation));
            }
            let actual = self.infer_expression_type(&expression)?;
            return expect_type(&actual, expected, label);
        }

        match (expected, value) {
            (WorkflowType::Array(item_type), Value::Array(items)) => {
                for (index, item) in items.iter().enumerate() {
                    let child_label = TypeViolation::label_with_path(label, &format!("[{index}]"));
                    self.expect_mixed_value_type(item, item_type, &child_label)?;
                }
                Ok(())
            }
            (WorkflowType::Map(value_type), Value::Object(object)) => {
                for (key, nested) in object {
                    let child_label = TypeViolation::label_with_path(label, &format!(".{key}"));
                    self.expect_mixed_value_type(nested, value_type, &child_label)?;
                }
                Ok(())
            }
            (WorkflowType::Struct { fields, additional }, Value::Object(object)) => {
                for (key, field) in fields {
                    let child_label = TypeViolation::label_with_path(label, &format!(".{key}"));
                    let Some(nested) = object.get(key) else {
                        if field.required {
                            return Err(type_error(
                                &child_label,
                                &TypeViolation::at(&[], field.ty.describe(), "missing"),
                            ));
                        }
                        continue;
                    };
                    if field.required && is_blank_parameter_value(nested) {
                        return Err(type_error(
                            &child_label,
                            &TypeViolation::at(&[], field.ty.describe(), "missing"),
                        ));
                    }
                    self.expect_mixed_value_type(nested, &field.ty, &child_label)?;
                }
                for (key, nested) in object {
                    if fields.contains_key(key) {
                        continue;
                    }
                    let child_label = TypeViolation::label_with_path(label, &format!(".{key}"));
                    let Some(additional) = additional else {
                        return Err(type_error(
                            &child_label,
                            &TypeViolation::at(&[], "no additional fields", "unexpected"),
                        ));
                    };
                    self.expect_mixed_value_type(nested, additional, &child_label)?;
                }
                Ok(())
            }
            _ => expected
                .validate_value(value)
                .map_err(|violation| type_error(label, &violation)),
        }
    }
}

// a required parameter must carry a concrete value. null, empty or
// whitespace-only strings, and empty arrays do not satisfy it. expression
// objects always count as provided since they resolve at runtime.
fn is_blank_parameter_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(text) => text.trim().is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => false,
    }
}

fn is_expression_object(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.contains_key("$ref")
        || object.contains_key("$concat")
        || object.contains_key("$coalesce")
        || object.contains_key("$literal")
        || object.contains_key("$to_string")
        || object.contains_key("$to_json_string")
        || object.contains_key("$call")
        || object.contains_key("$if")
        || object.contains_key("$lambda")
        || object.contains_key("$add")
        || object.contains_key("$sub")
        || object.contains_key("$mul")
        || object.contains_key("$div")
        || object.contains_key("$mod")
        || object.contains_key("$neg")
        || object.contains_key("$node")
        || object.contains_key("$value")
}

fn expect_type(
    actual: &WorkflowType,
    expected: &WorkflowType,
    label: &str,
) -> Result<(), WorkflowValidationError> {
    actual
        .validate_assignable_to(expected)
        .map_err(|violation| type_error(label, &violation))
}

fn assignable_type(
    actual: &WorkflowType,
    expected: &WorkflowType,
) -> Result<(), WorkflowValidationError> {
    actual
        .validate_assignable_to(expected)
        .map_err(|violation| WorkflowValidationError::TypeError(violation.to_string()))
}

fn type_error(label: &str, violation: &TypeViolation) -> WorkflowValidationError {
    WorkflowValidationError::TypeDiagnostic(WorkflowTypeDiagnostic {
        path: TypeViolation::label_with_path(label, &violation.path),
        expected: violation.expected.clone(),
        actual: violation.actual.clone(),
        message: violation.message_with_label(label),
    })
}

fn comparable_types(
    left: &WorkflowType,
    right: &WorkflowType,
) -> Result<(), WorkflowValidationError> {
    if left == right || (left.is_numeric() && right.is_numeric()) {
        return Ok(());
    }
    Err(WorkflowValidationError::TypeError(format!(
        "condition operands have incompatible types: {} and {}",
        left.describe(),
        right.describe()
    )))
}

fn common_type(left: WorkflowType, right: WorkflowType) -> Option<WorkflowType> {
    left.common_type(&right)
}

impl TypeContext {
    /// find a local bound to a first-class function type (a lambda value), if any.
    fn function_local(&self, name: &str) -> Option<&WorkflowType> {
        self.locals
            .iter()
            .rev()
            .find(|(local, ty)| local == name && matches!(ty, WorkflowType::Function { .. }))
            .map(|(_, ty)| ty)
    }
}

/// the statically-known string keys a lowered expression denotes, used to type key-driven
/// intrinsics (`at`/`pick`/`omit`): a string literal yields one key, a literal array of strings
/// yields several, anything else yields `None`.
fn static_string_keys(expr: &WorkflowExpression) -> Option<Vec<String>> {
    match expr {
        WorkflowExpression::Literal(Value::String(key)) => Some(vec![key.clone()]),
        WorkflowExpression::Literal(Value::Array(items)) => items
            .iter()
            .map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => None,
    }
}

fn workflow_context_type() -> WorkflowType {
    runinator_models::workflow_state::WorkflowContextHeader::runinator_type()
}
