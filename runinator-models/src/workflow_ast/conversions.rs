use super::*;

/// a malformed workflow expression rejected by the structural parser.
#[derive(Debug, Clone, PartialEq)]
pub struct InvalidExpression(pub String);

impl fmt::Display for InvalidExpression {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "invalid workflow expression: {}", self.0)
    }
}

impl std::error::Error for InvalidExpression {}

// -- value ref: structural parse/serialize ------------------------------------------------------

impl From<&WorkflowValueRef> for Value {
    fn from(reference: &WorkflowValueRef) -> Self {
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
            WorkflowRefSource::Input => single(REF_PARAMS, path),
            WorkflowRefSource::Prev => single(REF_PREV, path),
            WorkflowRefSource::Workflow => single(REF_WORKFLOW, path),
            WorkflowRefSource::Config => single(REF_CONFIG, path),
            WorkflowRefSource::Interrupt => single(REF_INTERRUPT, path),
            WorkflowRefSource::Local => single(REF_LOCAL, path),
            WorkflowRefSource::NodeOutput(node) => {
                let mut map = Map::new();
                map.insert(REF_NODE.into(), Value::String(node.as_str().to_string()));
                map.insert(REF_OUTPUT.into(), path);
                Value::Object(map)
            }
        }
    }
}

impl TryFrom<&Value> for WorkflowValueRef {
    type Error = InvalidExpression;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        let object = value.as_object().ok_or_else(|| invalid(value))?;
        if object.len() != 1
            && !(object.len() == 2
                && object.contains_key(REF_NODE)
                && object.contains_key(REF_OUTPUT))
        {
            return Err(invalid(value));
        }
        if let Some(path) = object.get(REF_PARAMS).or_else(|| object.get(REF_INPUT)) {
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
        if let Some(path) = object.get(REF_INTERRUPT) {
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::Interrupt,
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
                .ok_or_else(|| invalid(value))?;
            return Ok(WorkflowValueRef {
                source: WorkflowRefSource::NodeOutput(WorkflowNodeRef::new(node)),
                path: parse_path(output)?,
            });
        }
        Err(invalid(value))
    }
}

fn parse_path(value: &Value) -> Result<Vec<WorkflowPathSegment>, InvalidExpression> {
    let items = value.as_array().ok_or_else(|| invalid(value))?;
    items
        .iter()
        .map(|item| {
            if let Some(key) = item.as_str() {
                return Ok(WorkflowPathSegment::Key(key.to_string()));
            }
            if let Some(index) = item.as_u64() {
                return usize::try_from(index)
                    .map(WorkflowPathSegment::Index)
                    .map_err(|_| invalid(value));
            }
            Err(invalid(value))
        })
        .collect()
}

// -- expression: structural serialize -----------------------------------------------------------

impl From<&WorkflowExpression> for Value {
    fn from(expression: &WorkflowExpression) -> Self {
        match expression {
            WorkflowExpression::Literal(value) => value.clone(),
            WorkflowExpression::Ref(reference) => single(EXPR_REF, Value::from(reference)),
            WorkflowExpression::Concat(items) => single(EXPR_CONCAT, array(items)),
            WorkflowExpression::Coalesce(items) => single(EXPR_COALESCE, array(items)),
            WorkflowExpression::ToString(nested) => {
                single(EXPR_TO_STRING, Value::from(nested.as_ref()))
            }
            WorkflowExpression::ToJsonString(nested) => {
                single(EXPR_TO_JSON_STRING, Value::from(nested.as_ref()))
            }
            WorkflowExpression::Add(items) => single(EXPR_ADD, array(items)),
            WorkflowExpression::Sub(items) => single(EXPR_SUB, array(items)),
            WorkflowExpression::Mul(items) => single(EXPR_MUL, array(items)),
            WorkflowExpression::Div(items) => single(EXPR_DIV, array(items)),
            WorkflowExpression::Mod(items) => single(EXPR_MOD, array(items)),
            WorkflowExpression::Neg(nested) => single(EXPR_NEG, Value::from(nested.as_ref())),
            WorkflowExpression::Call { name, args } => {
                let mut map = Map::new();
                map.insert(EXPR_CALL.into(), Value::String(name.clone()));
                map.insert(EXPR_ARGS.into(), array(args));
                Value::Object(map)
            }
            WorkflowExpression::Lambda { params, body } => {
                let mut spec = Map::new();
                spec.insert(
                    LAMBDA_PARAMS.into(),
                    Value::Array(params.iter().map(|p| Value::String(p.clone())).collect()),
                );
                spec.insert(LAMBDA_BODY.into(), Value::from(body.as_ref()));
                single(EXPR_LAMBDA, Value::Object(spec))
            }
            WorkflowExpression::Apply { callee, args } => {
                let mut map = Map::new();
                map.insert(EXPR_APPLY.into(), Value::from(callee.as_ref()));
                map.insert(EXPR_ARGS.into(), array(args));
                Value::Object(map)
            }
            WorkflowExpression::Cond {
                condition,
                then,
                otherwise,
            } => {
                let mut map = Map::new();
                map.insert(EXPR_IF.into(), Value::from(condition.as_ref()));
                map.insert(EXPR_THEN.into(), Value::from(then.as_ref()));
                map.insert(EXPR_ELSE.into(), Value::from(otherwise.as_ref()));
                Value::Object(map)
            }
        }
    }
}

impl From<WorkflowExpression> for Value {
    fn from(expression: WorkflowExpression) -> Self {
        Value::from(&expression)
    }
}

fn array(items: &[WorkflowExpression]) -> Value {
    Value::Array(items.iter().map(Value::from).collect())
}

// -- expression: structural parse (validating) --------------------------------------------------

impl TryFrom<&Value> for WorkflowExpression {
    type Error = InvalidExpression;

    fn try_from(value: &Value) -> Result<Self, Self::Error> {
        match value {
            Value::Object(map) if map.contains_key(EXPR_VALUE) => Err(invalid(value)),
            Value::Object(map) if map.contains_key(EXPR_CALL) => {
                let name = map
                    .get(EXPR_CALL)
                    .and_then(Value::as_str)
                    .ok_or_else(|| invalid(value))?;
                if !map.keys().all(|key| key == EXPR_CALL || key == EXPR_ARGS) {
                    return Err(invalid(value));
                }
                let args = match map.get(EXPR_ARGS) {
                    None => Vec::new(),
                    Some(items) => items
                        .as_array()
                        .ok_or_else(|| invalid(value))?
                        .iter()
                        .map(WorkflowExpression::try_from)
                        .collect::<Result<Vec<_>, _>>()?,
                };
                Ok(WorkflowExpression::Call {
                    name: name.to_string(),
                    args,
                })
            }
            Value::Object(map) if map.contains_key(EXPR_APPLY) => {
                if !map.keys().all(|key| key == EXPR_APPLY || key == EXPR_ARGS) {
                    return Err(invalid(value));
                }
                let callee = map.get(EXPR_APPLY).ok_or_else(|| invalid(value))?;
                let args = match map.get(EXPR_ARGS) {
                    None => Vec::new(),
                    Some(items) => items
                        .as_array()
                        .ok_or_else(|| invalid(value))?
                        .iter()
                        .map(WorkflowExpression::try_from)
                        .collect::<Result<Vec<_>, _>>()?,
                };
                Ok(WorkflowExpression::Apply {
                    callee: Box::new(WorkflowExpression::try_from(callee)?),
                    args,
                })
            }
            Value::Object(map) if map.contains_key(EXPR_LAMBDA) => {
                if map.len() != 1 {
                    return Err(invalid(value));
                }
                let spec = map
                    .get(EXPR_LAMBDA)
                    .and_then(Value::as_object)
                    .ok_or_else(|| invalid(value))?;
                let params = spec
                    .get(LAMBDA_PARAMS)
                    .and_then(Value::as_array)
                    .ok_or_else(|| invalid(value))?
                    .iter()
                    .map(|param| {
                        param
                            .as_str()
                            .map(str::to_string)
                            .ok_or_else(|| invalid(value))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let body = spec.get(LAMBDA_BODY).ok_or_else(|| invalid(value))?;
                Ok(WorkflowExpression::Lambda {
                    params,
                    body: Box::new(WorkflowExpression::try_from(body)?),
                })
            }
            Value::Object(map) if map.contains_key(EXPR_IF) => {
                if !map
                    .keys()
                    .all(|key| key == EXPR_IF || key == EXPR_THEN || key == EXPR_ELSE)
                {
                    return Err(invalid(value));
                }
                let branch = |key: &str| {
                    map.get(key)
                        .ok_or_else(|| invalid(value))
                        .and_then(WorkflowExpression::try_from)
                        .map(Box::new)
                };
                Ok(WorkflowExpression::Cond {
                    condition: branch(EXPR_IF)?,
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
                    return Err(invalid(value));
                }
                if let Some(reference) = map.get(EXPR_REF) {
                    return Ok(WorkflowExpression::Ref(WorkflowValueRef::try_from(
                        reference,
                    )?));
                }
                for (key, ctor) in [
                    (
                        EXPR_ADD,
                        WorkflowExpression::Add
                            as fn(Vec<WorkflowExpression>) -> WorkflowExpression,
                    ),
                    (EXPR_SUB, WorkflowExpression::Sub),
                    (EXPR_MUL, WorkflowExpression::Mul),
                    (EXPR_DIV, WorkflowExpression::Div),
                    (EXPR_MOD, WorkflowExpression::Mod),
                ] {
                    if let Some(items) = map.get(key) {
                        let items = items
                            .as_array()
                            .filter(|items| !items.is_empty())
                            .ok_or_else(|| invalid(value))?;
                        return Ok(ctor(
                            items
                                .iter()
                                .map(WorkflowExpression::try_from)
                                .collect::<Result<Vec<_>, _>>()?,
                        ));
                    }
                }
                if let Some(operand) = map.get(EXPR_NEG) {
                    return Ok(WorkflowExpression::Neg(Box::new(
                        WorkflowExpression::try_from(operand)?,
                    )));
                }
                if let Some(items) = map.get(EXPR_CONCAT) {
                    let items = items.as_array().ok_or_else(|| invalid(value))?;
                    return Ok(WorkflowExpression::Concat(
                        items
                            .iter()
                            .map(WorkflowExpression::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
                if let Some(items) = map.get(EXPR_COALESCE) {
                    let items = items
                        .as_array()
                        .filter(|items| !items.is_empty())
                        .ok_or_else(|| invalid(value))?;
                    return Ok(WorkflowExpression::Coalesce(
                        items
                            .iter()
                            .map(WorkflowExpression::try_from)
                            .collect::<Result<Vec<_>, _>>()?,
                    ));
                }
                if let Some(literal) = map.get(EXPR_LITERAL) {
                    return Ok(WorkflowExpression::Literal(literal.clone()));
                }
                if let Some(nested) = map.get(EXPR_TO_STRING) {
                    return Ok(WorkflowExpression::ToString(Box::new(
                        WorkflowExpression::try_from(nested)?,
                    )));
                }
                if let Some(nested) = map.get(EXPR_TO_JSON_STRING) {
                    return Ok(WorkflowExpression::ToJsonString(Box::new(
                        WorkflowExpression::try_from(nested)?,
                    )));
                }
                Err(invalid(value))
            }
            Value::Object(map) => {
                let mut resolved = Map::new();
                for (key, nested) in map {
                    resolved.insert(
                        key.clone(),
                        Value::from(&WorkflowExpression::try_from(nested)?),
                    );
                }
                Ok(WorkflowExpression::Literal(Value::Object(resolved)))
            }
            Value::Array(items) => Ok(WorkflowExpression::Literal(Value::Array(
                items
                    .iter()
                    .map(|item| Ok(Value::from(&WorkflowExpression::try_from(item)?)))
                    .collect::<Result<Vec<_>, InvalidExpression>>()?,
            ))),
            Value::String(raw) if raw.contains("{{") || raw.contains("}}") => {
                Err(InvalidExpression(raw.clone()))
            }
            _ => Ok(WorkflowExpression::Literal(value.clone())),
        }
    }
}

impl TryFrom<Value> for WorkflowExpression {
    type Error = InvalidExpression;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        WorkflowExpression::try_from(&value)
    }
}

fn invalid(value: &Value) -> InvalidExpression {
    InvalidExpression(value.to_string())
}

// compute statement wire keys (`$if`/`then`/`else` are shared with the expression `Cond` form).
pub const STMT_LET: &str = "$let";
pub const STMT_VALUE: &str = "value";
pub const STMT_RETURN: &str = "$return";
pub const STMT_GOTO: &str = "$goto";

// -- compute program: structural serialize (inverse of `parse_program`) -------------------------

impl From<&ComputeStmt> for Value {
    fn from(statement: &ComputeStmt) -> Self {
        match statement {
            ComputeStmt::Let { name, value } => {
                let mut map = Map::new();
                map.insert(STMT_LET.into(), Value::String(name.clone()));
                map.insert(STMT_VALUE.into(), Value::from(value));
                Value::Object(map)
            }
            ComputeStmt::Return(expr) => single(STMT_RETURN, Value::from(expr)),
            ComputeStmt::Goto(target) => single(STMT_GOTO, Value::String(target.clone())),
            ComputeStmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let mut map = Map::new();
                map.insert(EXPR_IF.into(), Value::from(condition));
                map.insert(EXPR_THEN.into(), Value::from(then_branch));
                map.insert(EXPR_ELSE.into(), Value::from(else_branch));
                Value::Object(map)
            }
            // a bare expression statement serializes as the expression itself.
            ComputeStmt::Expr(expr) => Value::from(expr),
        }
    }
}

impl From<&ComputeProgram> for Value {
    fn from(program: &ComputeProgram) -> Self {
        Value::Array(program.0.iter().map(Value::from).collect())
    }
}

pub(super) fn single(key: &str, value: Value) -> Value {
    let mut map = Map::new();
    map.insert(key.to_string(), value);
    Value::Object(map)
}
