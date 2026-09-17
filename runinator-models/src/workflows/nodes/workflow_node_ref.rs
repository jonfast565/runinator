#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WorkflowNodeRef(pub(super) WorkflowNodeId);

impl WorkflowNodeRef {
    pub fn new(value: impl Into<String>) -> Self {
        Self(WorkflowNodeId::new(value))
    }

    pub fn id(&self) -> &WorkflowNodeId {
        &self.0
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    pub fn into_string(self) -> String {
        self.0.into_string()
    }
}

impl Serialize for WorkflowNodeRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(1))?;
        map.serialize_entry("$node", self.as_str())?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for WorkflowNodeRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let object = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("node reference must be an object"))?;
        if object.len() != 1 || !object.contains_key("$node") {
            return Err(serde::de::Error::custom(
                "node reference must be { \"$node\": \"node_id\" }",
            ));
        }
        let node = object
            .get("$node")
            .and_then(Value::as_str)
            .filter(|node| !node.is_empty())
            .ok_or_else(|| serde::de::Error::custom("$node must be a non-empty string"))?;
        Ok(Self::new(node))
    }
}
