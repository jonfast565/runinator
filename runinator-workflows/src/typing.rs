use std::collections::{BTreeMap, HashMap};

use runinator_models::{
    providers::{ActionMetadata, ParameterMetadata, ParameterValueType, ProviderMetadata},
    workflows::{WorkflowDefinition, WorkflowNode, WorkflowNodeKind},
};
use serde_json::Value;

use crate::{
    conditions::validate_condition,
    errors::WorkflowValidationError,
    expressions::{parse_expression, serialize_value_ref},
    parameters::{parse_map_parameters, parse_switch_parameters},
    types::{WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowType {
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array(Box<WorkflowType>),
    Object(BTreeMap<String, WorkflowType>),
    Json,
}

impl WorkflowType {
    fn object(fields: impl IntoIterator<Item = (impl Into<String>, WorkflowType)>) -> Self {
        Self::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.into(), value))
                .collect(),
        )
    }

    fn field(&self, key: &str) -> Option<&WorkflowType> {
        match self {
            WorkflowType::Object(fields) => fields.get(key),
            _ => None,
        }
    }

    fn is_numeric(&self) -> bool {
        matches!(self, WorkflowType::Integer | WorkflowType::Number)
    }

    fn is_primitive(&self) -> bool {
        matches!(
            self,
            WorkflowType::Boolean
                | WorkflowType::Integer
                | WorkflowType::Number
                | WorkflowType::String
        )
    }

    fn describe(&self) -> &'static str {
        match self {
            WorkflowType::Null => "null",
            WorkflowType::Boolean => "boolean",
            WorkflowType::Integer => "integer",
            WorkflowType::Number => "number",
            WorkflowType::String => "string",
            WorkflowType::Array(_) => "array",
            WorkflowType::Object(_) => "object",
            WorkflowType::Json => "json",
        }
    }
}

#[derive(Debug)]
struct TypeContext {
    input: WorkflowType,
    workflow: WorkflowType,
    node_outputs: HashMap<String, WorkflowType>,
}

pub fn validate_workflow_types(
    workflow: &WorkflowDefinition,
    nodes: &[WorkflowNode],
    providers: &[ProviderMetadata],
) -> Result<(), WorkflowValidationError> {
    let provider_actions = provider_actions(providers);
    let mut context = TypeContext {
        input: type_from_json_schema(&workflow.input_schema),
        workflow: workflow_context_type(),
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
        if matches!(node.kind, WorkflowNodeKind::Loop | WorkflowNodeKind::Map) {
            if let Some(output_type) = collection_node_output_type(node, &context)? {
                context
                    .node_outputs
                    .insert(node.id.as_str().to_string(), output_type);
            }
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

fn provider_actions<'a>(
    providers: &'a [ProviderMetadata],
) -> HashMap<(String, String), &'a ActionMetadata> {
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
            Ok(Some(WorkflowType::Object(
                metadata
                    .results
                    .iter()
                    .map(|result| {
                        let ty = result
                            .schema
                            .as_ref()
                            .map(type_from_json_schema)
                            .unwrap_or_else(|| type_from_parameter_value_type(result.value_type));
                        (result.name.clone(), ty)
                    })
                    .collect(),
            )))
        }
        WorkflowNodeKind::Subflow => Ok(Some(WorkflowType::object([
            ("subflow_run_id", WorkflowType::Integer),
            ("subflow_workflow_id", WorkflowType::Integer),
            ("run_name", WorkflowType::String),
            ("reused", WorkflowType::Boolean),
            ("status", WorkflowType::String),
            ("state", WorkflowType::Json),
            ("parameters", WorkflowType::Json),
        ]))),
        WorkflowNodeKind::Config => Ok(Some(WorkflowType::object([
            ("name", WorkflowType::String),
            ("metadata", WorkflowType::Json),
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
    Ok(Some(WorkflowType::object([
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
            if let Some(seconds) = node.wait.get("seconds") {
                expect_value_type(seconds, context, &WorkflowType::Integer, "wait.seconds")?;
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
        if param.required && !configuration.contains_key(&param.name) {
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
        let expected = parameter_type(param);
        expect_value_type(
            value,
            context,
            &expected,
            &format!("action parameter '{name}'"),
        )?;
    }
    Ok(())
}

fn parameter_type(param: &ParameterMetadata) -> WorkflowType {
    type_from_parameter_value_type(param.value_type)
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
                WorkflowType::Array(_) | WorkflowType::Object(_) | WorkflowType::Json
            ) {
                Ok(WorkflowType::String)
            } else {
                Err(WorkflowValidationError::TypeError(format!(
                    "$to_json_string requires an array, object, or json value, got {}",
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
                item_type.unwrap_or(WorkflowType::Json),
            )))
        }
        Value::Object(fields) => Ok(WorkflowType::Object(
            fields
                .iter()
                .map(|(key, value)| infer_value_type(value, context).map(|ty| (key.clone(), ty)))
                .collect::<Result<BTreeMap<_, _>, _>>()?,
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
        WorkflowRefSource::Prev => &WorkflowType::Json,
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
            (WorkflowPathSegment::Key(key), WorkflowType::Object(_)) => current.field(key)?,
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
    if let Some(all) = object.get("all") {
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
    if let Some(any) = object.get("any") {
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
    if let Some(not) = object.get("not") {
        validate_condition_types(not, context)?;
        return Ok(());
    }

    let left = object
        .get("value")
        .or_else(|| object.get("left"))
        .ok_or_else(|| WorkflowValidationError::InvalidCondition("missing value".into()))?;
    let left_type = infer_value_type(left, context)?;
    if let Some(expected) = object.get("equals") {
        comparable_types(&left_type, &infer_value_type(expected, context)?)?;
        return Ok(());
    }
    if let Some(expected) = object.get("not_equals") {
        comparable_types(&left_type, &infer_value_type(expected, context)?)?;
        return Ok(());
    }
    if let Some(expected) = object.get("contains") {
        let expected_type = infer_value_type(expected, context)?;
        return validate_contains_type(&left_type, &expected_type);
    }
    if let Some(expected) = object.get("in") {
        let expected_type = infer_value_type(expected, context)?;
        let WorkflowType::Array(item_type) = expected_type else {
            return Err(WorkflowValidationError::TypeError(
                "condition 'in' requires an array operand".into(),
            ));
        };
        assignable(&left_type, &item_type)?;
        return Ok(());
    }
    if let Some(expected) = object
        .get("starts_with")
        .or_else(|| object.get("ends_with"))
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
        .get("greater_than")
        .or_else(|| object.get("greater_than_or_equal"))
        .or_else(|| object.get("less_than"))
        .or_else(|| object.get("less_than_or_equal"))
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
    if let Some(expected) = object.get("exists") {
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
        WorkflowType::Array(item_type) => assignable(expected, item_type),
        WorkflowType::Object(_) => expect_type(expected, &WorkflowType::String, "object key"),
        _ => Err(WorkflowValidationError::TypeError(
            "contains requires a string, array, or object value".into(),
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

fn expect_type(
    actual: &WorkflowType,
    expected: &WorkflowType,
    label: &str,
) -> Result<(), WorkflowValidationError> {
    assignable(actual, expected).map_err(|_| {
        WorkflowValidationError::TypeError(format!(
            "{label} expected {}, got {}",
            expected.describe(),
            actual.describe()
        ))
    })
}

fn assignable(
    actual: &WorkflowType,
    expected: &WorkflowType,
) -> Result<(), WorkflowValidationError> {
    if actual == expected || matches!(expected, WorkflowType::Json) {
        return Ok(());
    }
    if matches!(
        (actual, expected),
        (WorkflowType::Integer, WorkflowType::Number)
    ) {
        return Ok(());
    }
    match (actual, expected) {
        (WorkflowType::Array(actual), WorkflowType::Array(expected)) => {
            assignable(actual, expected)
        }
        (WorkflowType::Object(actual), WorkflowType::Object(expected)) => {
            for (key, expected_type) in expected {
                let Some(actual_type) = actual.get(key) else {
                    return Err(WorkflowValidationError::TypeError(format!(
                        "object is missing field '{key}'"
                    )));
                };
                assignable(actual_type, expected_type)?;
            }
            Ok(())
        }
        _ => Err(WorkflowValidationError::TypeError(format!(
            "expected {}, got {}",
            expected.describe(),
            actual.describe()
        ))),
    }
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
    WorkflowType::object([
        ("run_id", WorkflowType::Integer),
        ("workflow_id", WorkflowType::Integer),
        ("name", WorkflowType::String),
        ("state", WorkflowType::Json),
    ])
}

fn type_from_parameter_value_type(value_type: ParameterValueType) -> WorkflowType {
    match value_type {
        ParameterValueType::String => WorkflowType::String,
        ParameterValueType::Integer => WorkflowType::Integer,
        ParameterValueType::Number => WorkflowType::Number,
        ParameterValueType::Boolean => WorkflowType::Boolean,
        ParameterValueType::StringArray => WorkflowType::Array(Box::new(WorkflowType::String)),
        ParameterValueType::NumberArray => WorkflowType::Array(Box::new(WorkflowType::Number)),
        ParameterValueType::Object => WorkflowType::Object(BTreeMap::new()),
        ParameterValueType::Json => WorkflowType::Json,
    }
}

fn type_from_json_schema(schema: &Value) -> WorkflowType {
    let Some(object) = schema.as_object() else {
        return WorkflowType::Json;
    };
    let schema_type = object.get("type").and_then(Value::as_str);
    if schema_type.is_none() && object.contains_key("properties") {
        return object_schema_type(object);
    }
    match schema_type {
        Some("null") => WorkflowType::Null,
        Some("boolean") => WorkflowType::Boolean,
        Some("integer") => WorkflowType::Integer,
        Some("number") => WorkflowType::Number,
        Some("string") => WorkflowType::String,
        Some("array") => WorkflowType::Array(Box::new(
            object
                .get("items")
                .map(type_from_json_schema)
                .unwrap_or(WorkflowType::Json),
        )),
        Some("object") => object_schema_type(object),
        _ => WorkflowType::Json,
    }
}

fn object_schema_type(object: &serde_json::Map<String, Value>) -> WorkflowType {
    let fields = object
        .get("properties")
        .and_then(Value::as_object)
        .map(|properties| {
            properties
                .iter()
                .map(|(key, value)| (key.clone(), type_from_json_schema(value)))
                .collect()
        })
        .unwrap_or_default();
    WorkflowType::Object(fields)
}
