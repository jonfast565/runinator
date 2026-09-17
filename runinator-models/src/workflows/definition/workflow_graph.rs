#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowGraph {
    #[serde(default)]
    pub start: Option<String>,
    #[serde(default)]
    pub nodes: Vec<WorkflowNode>,
    #[serde(default, rename = "$defs")]
    pub defs: Map,
    #[serde(default)]
    pub metadata: Value,
    #[serde(flatten)]
    pub extra: Map,
}

impl WorkflowGraph {
    pub fn as_value(&self) -> Value {
        serde_json::to_value(self)
            .map(Value::from)
            .unwrap_or_else(|_| Value::Object(Map::new()))
    }

    pub fn from_value(value: Value) -> Result<Self, String> {
        match serde_json::from_value(value.clone().into()) {
            Ok(graph) => Ok(graph),
            Err(_) => {
                let mut expanded = value;
                expand_local_defs_refs(&mut expanded, &mut Vec::new())?;
                serde_json::from_value(expanded.into()).map_err(|err| err.to_string())
            }
        }
    }
}

impl fmt::Display for WorkflowGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_value().fmt(formatter)
    }
}
