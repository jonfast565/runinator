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

/// which terminal state of a source member fires the link to the next member. mirrors the `on`
/// selector of a `chained` workflow trigger (`success` / `complete` / `failure`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineLinkSelector {
    /// fire only when the source run succeeds.
    #[default]
    Success,
    /// fire when the source run reaches any terminal state.
    Complete,
    /// fire only when the source run fails or times out.
    Failure,
}

impl PipelineLinkSelector {
    /// the chained-trigger `on` string this selector maps to.
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineLinkSelector::Success => "success",
            PipelineLinkSelector::Complete => "complete",
            PipelineLinkSelector::Failure => "failure",
        }
    }
}

/// a directed link between two member workflows (by name), realized as a `chained` trigger on the
/// `from` workflow targeting the `to` workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineLinkSpec {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub on: PipelineLinkSelector,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// a portable, id-free pipeline declaration compiled from a `.wdlp` file. members and links are by
/// workflow name; the web service resolves names to ids at import and materializes the links as
/// managed `chained` triggers stamped with the upserted pipeline's id.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub defaults: PipelineDefaults,
    #[serde(default)]
    pub members: Vec<String>,
    #[serde(default)]
    pub links: Vec<PipelineLinkSpec>,
}

// PipelineDefaults derives Clone but not PartialEq; PipelineSpec's PartialEq needs it.
impl PartialEq for PipelineDefaults {
    fn eq(&self, other: &Self) -> bool {
        self.on_step_failure == other.on_step_failure
            && self.links_enabled_by_default == other.links_enabled_by_default
            && self.default_parameters == other.default_parameters
            && self.max_chain_depth == other.max_chain_depth
    }
}

/// the compiled pipeline artifact carried in a pack zip as `pipelines.json`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PipelineBundle {
    #[serde(default)]
    pub pipelines: Vec<PipelineSpec>,
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
