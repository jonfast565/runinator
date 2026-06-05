use std::collections::HashMap;

use runinator_models::value::Value;
use runinator_models::{
    providers::{ActionMetadata, ParameterMetadata, ProviderMetadata, validate_provider_metadata},
    types::{RuninatorType, TypeViolation},
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeKind, WorkflowWaitSeconds},
};

use crate::{
    conditions::validate_condition,
    errors::{WorkflowTypeDiagnostic, WorkflowValidationError},
    expressions::{parse_expression, serialize_value_ref},
    keys::{
        COND_ALL, COND_ANY, COND_CONTAINS, COND_ENDS_WITH, COND_EQUALS, COND_EXISTS,
        COND_GREATER_THAN, COND_GREATER_THAN_OR_EQUAL, COND_IN, COND_LEFT, COND_LESS_THAN,
        COND_LESS_THAN_OR_EQUAL, COND_NOT, COND_NOT_EQUALS, COND_STARTS_WITH, COND_VALUE,
    },
    parameters::{parse_map_parameters, parse_switch_parameters},
    types::{WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef},
};

pub type WorkflowType = RuninatorType;

#[derive(Debug)]
struct TypeContext {
    input: WorkflowType,
    workflow: WorkflowType,
    config: WorkflowType,
    node_outputs: HashMap<String, WorkflowType>,
}

pub fn validate_workflow_types(
    workflow: &WorkflowDefinition,
    nodes: &[WorkflowNode],
    providers: &[ProviderMetadata],
    config_type: &WorkflowType,
) -> Result<(), WorkflowValidationError> {
    let provider_actions = provider_actions(providers);
    validate_provider_metadata_set(providers)?;
    let mut context = TypeContext {
        input: workflow.input_type.clone(),
        workflow: workflow_context_type(),
        config: config_type.clone(),
        node_outputs: HashMap::new(),
    };

    for node in nodes {
        if let Some(output_type) = static_node_output_type(node, &provider_actions)? {
            context
                .node_outputs
                .insert(node.id.as_str().to_string(), output_type);
        }
    }
    for node in nodes {
        if matches!(node.kind, WorkflowNodeKind::Loop | WorkflowNodeKind::Map)
            && let Some(output_type) = collection_node_output_type(node, &context)?
        {
            context
                .node_outputs
                .insert(node.id.as_str().to_string(), output_type);
        }
    }

    for node in nodes {
        validate_condition_types(&node.condition, &context)?;
        for branch in &node.transitions.branches {
            validate_condition_types(&branch.when, &context)?;
        }
        validate_node_types(node, &context, &provider_actions)?;
    }

    Ok(())
}

fn validate_provider_metadata_set(
    providers: &[ProviderMetadata],
) -> Result<(), WorkflowValidationError> {
    for provider in providers {
        validate_provider_metadata(provider).map_err(WorkflowValidationError::TypeError)?;
    }
    Ok(())
}

fn provider_actions(providers: &[ProviderMetadata]) -> HashMap<(String, String), &ActionMetadata> {
    providers
        .iter()
        .flat_map(|provider| {
            provider.actions.iter().map(move |action| {
                (
                    (provider.name.clone(), action.function_name.clone()),
                    action,
                )
            })
        })
        .collect()
}

fn static_node_output_type(
    node: &WorkflowNode,
    provider_actions: &HashMap<(String, String), &ActionMetadata>,
) -> Result<Option<WorkflowType>, WorkflowValidationError> {
    match node.kind {
        WorkflowNodeKind::Action => {
            let action = node.action.as_ref().ok_or_else(|| {
                WorkflowValidationError::MissingAction(node.id.as_str().to_string())
            })?;
            let metadata = provider_actions
                .get(&(action.provider.clone(), action.function.clone()))
                .ok_or_else(|| {
                    WorkflowValidationError::TypeError(format!(
                        "node '{}' references unknown provider action '{}.{}'",
                        node.id, action.provider, action.function
                    ))
                })?;
            Ok(Some(WorkflowType::structure(
                metadata
                    .results
                    .iter()
                    .map(|result| (result.name.clone(), result.ty.clone())),
            )))
        }
        WorkflowNodeKind::Subflow => Ok(Some(WorkflowType::structure([
            ("subflow_run_id", WorkflowType::Integer),
            ("subflow_workflow_id", WorkflowType::Integer),
            ("run_name", WorkflowType::String),
            ("reused", WorkflowType::Boolean),
            ("status", WorkflowType::String),
            ("state", WorkflowType::Any),
            ("parameters", WorkflowType::Any),
        ]))),
        WorkflowNodeKind::Config => Ok(Some(WorkflowType::structure([
            ("name", WorkflowType::String),
            ("metadata", WorkflowType::Any),
        ]))),
        _ => Ok(None),
    }
}

fn collection_node_output_type(
    node: &WorkflowNode,
    context: &TypeContext,
) -> Result<Option<WorkflowType>, WorkflowValidationError> {
    let items = match node.kind {
        WorkflowNodeKind::Loop => node.parameters.get("items"),
        WorkflowNodeKind::Map => Some(&parse_map_parameters(node)?.items),
        _ => None,
    };
    let Some(items) = items else {
        return Ok(None);
    };
    let items_type = infer_value_type(items, context)?;
    let WorkflowType::Array(item_type) = items_type else {
        return Err(WorkflowValidationError::TypeError(format!(
            "node '{}' items must be an array, got {}",
            node.id,
            items_type.describe()
        )));
    };
    Ok(Some(WorkflowType::structure([
        ("item", *item_type),
        ("index", WorkflowType::Integer),
    ])))
}

fn validate_node_types(
    node: &WorkflowNode,
    context: &TypeContext,
    provider_actions: &HashMap<(String, String), &ActionMetadata>,
) -> Result<(), WorkflowValidationError> {
    match node.kind {
        WorkflowNodeKind::Action => validate_action_configuration(node, context, provider_actions),
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
                        expect_value_type(
                            expression.as_value(),
                            context,
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
            infer_value_type(&params.value, context)?;
            for case in params.cases {
                validate_condition_types(&case.condition, context)?;
            }
            Ok(())
        }
        WorkflowNodeKind::Loop => {
            let Some(items) = node.parameters.get("items") else {
                return Err(WorkflowValidationError::InvalidNodeParameters {
                    node: node.id.as_str().to_string(),
                    message: "loop.items is required".into(),
                });
            };
            let ty = infer_value_type(items, context)?;
            if !matches!(ty, WorkflowType::Array(_)) {
                return Err(WorkflowValidationError::TypeError(format!(
                    "node '{}' loop.items must be an array, got {}",
                    node.id,
                    ty.describe()
                )));
            }
            Ok(())
        }
        WorkflowNodeKind::Map => {
            let params = parse_map_parameters(node)?;
            let ty = infer_value_type(&params.items, context)?;
            if !matches!(ty, WorkflowType::Array(_)) {
                return Err(WorkflowValidationError::TypeError(format!(
                    "node '{}' map.items must be an array, got {}",
                    node.id,
                    ty.describe()
                )));
            }
            Ok(())
        }
        WorkflowNodeKind::Subflow => {
            if let Some(run_name) = node.subflow.run_name.as_ref() {
                expect_value_type(run_name, context, &WorkflowType::String, "subflow.run_name")?;
            }
            infer_value_type(&node.parameters, context)?;
            Ok(())
        }
        WorkflowNodeKind::Config => {
            if let Some(name) = node.parameters.get("name") {
                expect_value_type(name, context, &WorkflowType::String, "config.name")?;
            }
            if let Some(metadata) = node.parameters.get("metadata") {
                infer_value_type(metadata, context)?;
            }
            Ok(())
        }
        WorkflowNodeKind::Approval => {
            if let Some(approval_type) = node.parameters.get("approval_type") {
                expect_value_type(
                    approval_type,
                    context,
                    &WorkflowType::String,
                    "approval.approval_type",
                )?;
            }
            if let Some(prompt) = node.parameters.get("prompt") {
                expect_value_type(prompt, context, &WorkflowType::String, "approval.prompt")?;
            }
            Ok(())
        }
        WorkflowNodeKind::Emit => {
            if let Some(event_type) = node.parameters.get("event_type") {
                expect_value_type(
                    event_type,
                    context,
                    &WorkflowType::String,
                    "emit.event_type",
                )?;
            }
            if let Some(data) = node.parameters.get("data") {
                infer_value_type(data, context)?;
            }
            Ok(())
        }
        _ => {
            infer_value_type(&node.parameters, context)?;
            Ok(())
        }
    }
}

fn validate_action_configuration(
    node: &WorkflowNode,
    context: &TypeContext,
    provider_actions: &HashMap<(String, String), &ActionMetadata>,
) -> Result<(), WorkflowValidationError> {
    let action = node
        .action
        .as_ref()
        .ok_or_else(|| WorkflowValidationError::MissingAction(node.id.as_str().to_string()))?;
    let metadata = provider_actions
        .get(&(action.provider.clone(), action.function.clone()))
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
        expect_parameter_value_type(value, context, &parameter_type(param), name)?;
    }
    Ok(())
}

fn parameter_type(param: &ParameterMetadata) -> WorkflowType {
    param.ty.clone()
}

fn infer_value_type(
    value: &Value,
    context: &TypeContext,
) -> Result<WorkflowType, WorkflowValidationError> {
    infer_expression_type(&parse_expression(value)?, context)
}

fn infer_expression_type(
    expression: &WorkflowExpression,
    context: &TypeContext,
) -> Result<WorkflowType, WorkflowValidationError> {
    match expression {
        WorkflowExpression::Literal(value) => literal_type(value, context),
        WorkflowExpression::Ref(reference) => resolve_ref_type(reference, context),
        WorkflowExpression::Concat(items) => {
            for item in items {
                let ty = infer_expression_type(item, context)?;
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
            let mut resolved = None;
            for item in items {
                let ty = infer_expression_type(item, context)?;
                if ty == WorkflowType::Null {
                    continue;
                }
                resolved = Some(match resolved {
                    None => ty,
                    Some(existing) => common_type(existing, ty).unwrap_or(WorkflowType::Any),
                });
            }
            Ok(resolved.unwrap_or(WorkflowType::Null))
        }
        WorkflowExpression::ToString(nested) => {
            let ty = infer_expression_type(nested, context)?;
            if ty.is_primitive() {
                Ok(WorkflowType::String)
            } else {
                Err(WorkflowValidationError::TypeError(format!(
                    "$to_string requires a primitive value, got {}",
                    ty.describe()
                )))
            }
        }
        WorkflowExpression::ToJsonString(nested) => {
            let ty = infer_expression_type(nested, context)?;
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
    }
}

fn literal_type(
    value: &Value,
    context: &TypeContext,
) -> Result<WorkflowType, WorkflowValidationError> {
    match value {
        Value::Null => Ok(WorkflowType::Null),
        Value::Bool(_) => Ok(WorkflowType::Boolean),
        Value::Number(number) if number.is_i64() || number.is_u64() => Ok(WorkflowType::Integer),
        Value::Number(_) => Ok(WorkflowType::Number),
        Value::String(_) => Ok(WorkflowType::String),
        Value::Array(items) => {
            let mut item_type = None;
            for item in items {
                let ty = infer_value_type(item, context)?;
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
                .map(|(key, value)| infer_value_type(value, context).map(|ty| (key.clone(), ty)))
                .collect::<Result<Vec<_>, _>>()?,
        )),
    }
}

fn resolve_ref_type(
    reference: &WorkflowValueRef,
    context: &TypeContext,
) -> Result<WorkflowType, WorkflowValidationError> {
    let base = match &reference.source {
        WorkflowRefSource::Input => &context.input,
        WorkflowRefSource::Workflow => &context.workflow,
        WorkflowRefSource::Prev => &WorkflowType::Any,
        // config is typed from the stored settings schema (`{ scope: { name: type } }`); an
        // open struct keeps not-yet-configured keys permissive (`any`) instead of erroring.
        WorkflowRefSource::Config => &context.config,
        WorkflowRefSource::NodeOutput(node) => {
            context.node_outputs.get(node.as_str()).ok_or_else(|| {
                WorkflowValidationError::MissingRef(serialize_value_ref(reference).to_string())
            })?
        }
    };
    resolve_path_type(base, &reference.path)
        .cloned()
        .ok_or_else(|| {
            WorkflowValidationError::MissingRef(serialize_value_ref(reference).to_string())
        })
}

fn resolve_path_type<'a>(
    base: &'a WorkflowType,
    path: &[WorkflowPathSegment],
) -> Option<&'a WorkflowType> {
    let mut current = base;
    for segment in path {
        current = match (segment, current) {
            // an `any` base absorbs any path: drilling into the unknown stays unknown.
            (_, WorkflowType::Any) => return Some(&WorkflowType::Any),
            (WorkflowPathSegment::Key(key), WorkflowType::Struct { .. } | WorkflowType::Map(_)) => {
                current.field(key)?
            }
            (WorkflowPathSegment::Index(_), WorkflowType::Array(item)) => item,
            _ => return None,
        };
    }
    Some(current)
}

fn validate_condition_types(
    condition: &Value,
    context: &TypeContext,
) -> Result<(), WorkflowValidationError> {
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
            validate_condition_types(item, context)?;
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
            validate_condition_types(item, context)?;
        }
        return Ok(());
    }
    if let Some(not) = object.get(COND_NOT) {
        validate_condition_types(not, context)?;
        return Ok(());
    }

    let left = object
        .get(COND_VALUE)
        .or_else(|| object.get(COND_LEFT))
        .ok_or_else(|| WorkflowValidationError::InvalidCondition("missing value".into()))?;
    let left_type = infer_value_type(left, context)?;
    if let Some(expected) = object.get(COND_EQUALS) {
        comparable_types(&left_type, &infer_value_type(expected, context)?)?;
        return Ok(());
    }
    if let Some(expected) = object.get(COND_NOT_EQUALS) {
        comparable_types(&left_type, &infer_value_type(expected, context)?)?;
        return Ok(());
    }
    if let Some(expected) = object.get(COND_CONTAINS) {
        let expected_type = infer_value_type(expected, context)?;
        return validate_contains_type(&left_type, &expected_type);
    }
    if let Some(expected) = object.get(COND_IN) {
        let expected_type = infer_value_type(expected, context)?;
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
            &infer_value_type(expected, context)?,
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
        let right_type = infer_value_type(expected, context)?;
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
        expect_value_type(expected, context, &WorkflowType::Boolean, "exists")?;
        return Ok(());
    }
    Ok(())
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

fn expect_value_type(
    value: &Value,
    context: &TypeContext,
    expected: &WorkflowType,
    label: &str,
) -> Result<(), WorkflowValidationError> {
    let actual = infer_value_type(value, context)?;
    expect_type(&actual, expected, label)
}

fn expect_parameter_value_type(
    value: &Value,
    context: &TypeContext,
    expected: &WorkflowType,
    name: &str,
) -> Result<(), WorkflowValidationError> {
    let label = format!("action parameter '{name}'");
    expect_mixed_value_type(value, context, expected, &label)
}

fn expect_mixed_value_type(
    value: &Value,
    context: &TypeContext,
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
        let actual = infer_expression_type(&expression, context)?;
        return expect_type(&actual, expected, label);
    }

    match (expected, value) {
        (WorkflowType::Array(item_type), Value::Array(items)) => {
            for (index, item) in items.iter().enumerate() {
                let child_label = TypeViolation::label_with_path(label, &format!("[{index}]"));
                expect_mixed_value_type(item, context, item_type, &child_label)?;
            }
            Ok(())
        }
        (WorkflowType::Map(value_type), Value::Object(object)) => {
            for (key, nested) in object {
                let child_label = TypeViolation::label_with_path(label, &format!(".{key}"));
                expect_mixed_value_type(nested, context, value_type, &child_label)?;
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
                expect_mixed_value_type(nested, context, &field.ty, &child_label)?;
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
                expect_mixed_value_type(nested, context, additional, &child_label)?;
            }
            Ok(())
        }
        _ => expected
            .validate_value(value)
            .map_err(|violation| type_error(label, &violation)),
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
    if left == right {
        return Some(left);
    }
    if left.is_numeric() && right.is_numeric() {
        return Some(WorkflowType::Number);
    }
    None
}

fn workflow_context_type() -> WorkflowType {
    WorkflowType::structure([
        ("run_id", WorkflowType::Integer),
        ("workflow_id", WorkflowType::Integer),
        ("name", WorkflowType::String),
        ("state", WorkflowType::Any),
    ])
}
