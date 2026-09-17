#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowObject(pub(super) Value);

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
