use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub id: Option<Uuid>,
    pub name: String,
    /// Stable authoring key for this logical workflow. Display-name edits and namespace moves do
    /// not change it. Older definitions omit it and temporarily fall back to `name` until the
    /// namespace migration writes an explicit key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// the namespace that qualifies this workflow's identity, from a `namespace <path>` header.
    /// `None` for an unqualified workflow. a subflow target `"<namespace>.<name>"` resolves against
    /// the qualified identity `namespace + "." + name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// the organization (tenant) that owns this workflow. `None` means platform-global / unassigned,
    /// which keeps pre-tenancy workflows working unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub version: SemVer,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    #[serde(deserialize_with = "deserialize_workflow_type")]
    pub input_type: RuninatorType,
    #[serde(default)]
    pub definition: WorkflowGraph,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

impl WorkflowDefinition {
    /// The durable key used when source does not carry the UUID directly.
    pub fn artifact_key(&self) -> &str {
        self.key.as_deref().unwrap_or(&self.name)
    }

    /// The current human-facing path. This is an alias for the UUID, not the artifact identity.
    pub fn artifact_path(&self) -> crate::artifacts::ArtifactPath {
        crate::artifacts::ArtifactPath::new(self.namespace.clone(), self.artifact_key().to_string())
    }
}

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

fn expand_local_defs_refs(value: &mut Value, stack: &mut Vec<String>) -> Result<(), String> {
    let defs = value
        .get("$defs")
        .cloned()
        .unwrap_or_else(|| Value::Object(Map::new()));
    expand_refs_in_value(value, &defs, stack)
}

fn expand_refs_in_value(
    value: &mut Value,
    defs: &Value,
    stack: &mut Vec<String>,
) -> Result<(), String> {
    match value {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_string)
                && let Some(pointer) = reference.strip_prefix("#/$defs/")
            {
                if stack.iter().any(|item| item == &reference) {
                    return Err(format!("detected local $ref cycle for '{reference}'"));
                }
                let path = format!("/{pointer}");
                let mut replacement = defs
                    .pointer(&path)
                    .cloned()
                    .ok_or_else(|| format!("missing local $ref '{reference}'"))?;
                stack.push(reference.clone());
                expand_refs_in_value(&mut replacement, defs, stack)?;
                stack.pop();
                for (key, overlay) in map.clone() {
                    if key != "$ref"
                        && key != "with"
                        && let Value::Object(replacement_map) = &mut replacement
                    {
                        replacement_map.insert(key, overlay);
                    }
                }
                if let Some(with) = map.get("with") {
                    merge_overlay(&mut replacement, with.clone());
                }
                *value = replacement;
                return Ok(());
            }
            for nested in map.values_mut() {
                expand_refs_in_value(nested, defs, stack)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                expand_refs_in_value(item, defs, stack)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn merge_overlay(target: &mut Value, overlay: Value) {
    match (target, overlay) {
        (Value::Object(target), Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(&key) {
                    Some(existing) => merge_overlay(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}

impl fmt::Display for WorkflowGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_value().fmt(formatter)
    }
}

fn deserialize_workflow_type<'de, D>(deserializer: D) -> Result<RuninatorType, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    serde_json::from_value(value.clone().into())
        .or_else(|_| Ok(RuninatorType::from_json_schema(&value)))
}

/// request body for duplicating a workflow into a new version sharing the same name.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowDuplicateRequest {
    #[serde(default)]
    pub bump: SemVerBump,
}

/// request body for a server-side dry-run (branch preview). The `workflow` is walked with the
/// reducer's evaluators against live config, publishing no actions; `inputs` seed the run and an
/// optional `replay_run` replays that run's recorded node outputs so the walk follows real branches.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowSimulateRequest {
    pub workflow: WorkflowDefinition,
    #[serde(default)]
    pub inputs: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay_run: Option<Uuid>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowBundle {
    #[serde(default)]
    pub workflows: Vec<WorkflowDefinition>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
}

impl crate::validation::Validate for WorkflowDefinition {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        use crate::validation::{
            SHORT_TEXT_MAX, identifier, optional_text, required_text, serialized,
        };

        required_text("name", &self.name, SHORT_TEXT_MAX)?;
        if let Some(key) = self.key.as_deref() {
            identifier("key", key)?;
        }
        optional_text("namespace", self.namespace.as_deref(), SHORT_TEXT_MAX)?;
        serialized("workflow", self)?;
        Ok(())
    }
}

impl crate::validation::Validate for WorkflowSimulateRequest {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        crate::validation::Validate::validate(&self.workflow)?;
        crate::validation::dynamic_value("inputs", &self.inputs)?;
        Ok(())
    }
}

// note: raw json workflow bundles use an explicit client method because the server requires
// a risk-acknowledgment header before accepting them.
