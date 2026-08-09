use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowObject(Value);

impl WorkflowObject {
    pub fn as_value(&self) -> &Value {
        &self.0
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    pub fn into_object(self) -> Map {
        self.0.as_object().cloned().unwrap_or_default()
    }

    pub fn from_value(value: Value) -> Result<Self, String> {
        match value {
            Value::Null => Ok(Self(Value::Object(Map::new()))),
            Value::Object(_) => Ok(Self(value)),
            _ => Err("value must be an object".into()),
        }
    }
}

impl Default for WorkflowObject {
    fn default() -> Self {
        Self(Value::Object(Map::new()))
    }
}

impl Deref for WorkflowObject {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.as_value()
    }
}

impl Serialize for WorkflowObject {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowObject {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        WorkflowObject::from_value(value).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for WorkflowObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl From<WorkflowObject> for Value {
    fn from(value: WorkflowObject) -> Self {
        value.into_value()
    }
}

/// a node/branch condition: a typed `ConditionNode` tree, or `None` for the null "always true" case.
/// serializes through `Value` so the wire json is byte-identical to the untyped form it replaced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WorkflowCondition(Option<ConditionNode>);

impl WorkflowCondition {
    /// the typed condition, or `None` when the condition is null (unconditional).
    pub fn node(&self) -> Option<&ConditionNode> {
        self.0.as_ref()
    }

    /// whether there is no condition (the null, always-true case).
    pub fn is_empty(&self) -> bool {
        self.0.is_none()
    }

    /// the wire `Value` form: null when empty, otherwise the condition object.
    pub fn to_value(&self) -> Value {
        match &self.0 {
            None => Value::Null,
            Some(node) => Value::from(node),
        }
    }

    /// build from a wire value: null yields the empty (always-true) condition; an object is parsed
    /// into the typed tree (unknown shapes are preserved verbatim by `ConditionNode`).
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Null => Self(None),
            other => Self(Some(ConditionNode::from(&other))),
        }
    }
}

impl From<WorkflowCondition> for Value {
    fn from(value: WorkflowCondition) -> Self {
        value.to_value()
    }
}

impl Serialize for WorkflowCondition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for WorkflowCondition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::Null | Value::Object(_) => Ok(Self::from_value(value)),
            _ => Err(serde::de::Error::custom(
                "condition must be null or an object",
            )),
        }
    }
}

impl fmt::Display for WorkflowCondition {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_value().fmt(formatter)
    }
}
