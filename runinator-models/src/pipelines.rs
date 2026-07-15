use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

/// what happens to downstream links when a member workflow fails. authoring-only: it seeds the
/// `on` selector of newly drawn links (`Halt` -> fire on success, `Continue` -> fire on complete).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineFailurePolicy {
    /// stop the pipeline when a step fails (new links default to firing on success).
    #[default]
    Halt,
    /// keep going when a step fails (new links default to firing on complete).
    Continue,
}

impl PipelineFailurePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineFailurePolicy::Halt => "halt",
            PipelineFailurePolicy::Continue => "continue",
        }
    }
}

/// editable pipeline-level defaults applied when authoring links inside a pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineDefaults {
    #[serde(default)]
    pub on_step_failure: PipelineFailurePolicy,
    #[serde(default = "default_true")]
    pub links_enabled_by_default: bool,
    #[serde(default)]
    pub default_parameters: Value,
    #[serde(default)]
    pub max_chain_depth: Option<u32>,
}

fn default_true() -> bool {
    true
}

impl Default for PipelineDefaults {
    fn default() -> Self {
        PipelineDefaults {
            on_step_failure: PipelineFailurePolicy::default(),
            links_enabled_by_default: true,
            default_parameters: Value::default(),
            max_chain_depth: None,
        }
    }
}

/// a named pipeline instance: a chosen set of member workflows plus authoring defaults. the links
/// between members remain `chained` workflow triggers stamped with this pipeline's id; the runtime
/// chaining engine is unaware of pipelines.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pipeline {
    pub id: Option<Uuid>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// the owning organization (tenant), or `None` for platform-global. stamped from the creator's
    /// active org on create and preserved on update.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub workflow_ids: Vec<Uuid>,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}
