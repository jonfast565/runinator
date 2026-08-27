//! UI metadata catalogs for workflow node kinds, edges, and triggers.
//!
//! this is the single source of truth the command center renders from (palette, generic step
//! editor, read-only detail view, edge palette, trigger forms). the node-kind half is assembled
//! from [`crate::node_kinds`], where each kind's palette entry sits next to the parameter parsing,
//! validation, and graph rules it has to agree with. adding a node kind is a change there plus the
//! model enum — the frontend needs no per-kind edits.

use runinator_models::catalog_metadata::{
    EnumCatalogMetadata, EnumOptionMetadata, UiField, WorkflowNodeKindMetadata,
    WorkflowTriggerKindMetadata,
};
use runinator_models::interrupt::{InterruptMode, InterruptSource};
use runinator_models::json;
use runinator_models::providers::{ParameterMetadata, RuninatorType};
use runinator_models::schedules::ConcurrencyPolicy;
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind, WorkflowTriggerKind};

use crate::node_kinds::{ActionCatalog, spec_for};

// -- the node catalog --------------------------------------------------------------------------

/// ordered UI metadata for every workflow node kind.
pub fn node_kind_catalog() -> Vec<WorkflowNodeKindMetadata> {
    WorkflowNodeKind::ALL.iter().map(node_metadata).collect()
}

/// the palette entry for one node kind.
pub fn node_metadata(kind: &WorkflowNodeKind) -> WorkflowNodeKindMetadata {
    let spec = spec_for(kind);
    let mut metadata = spec.metadata();
    let mut template = metadata.default_template.clone();
    if let Value::Object(object) = &mut template {
        object.insert("id".into(), Value::String("catalog_node".into()));
    }
    let providers = Vec::new();
    let actions = ActionCatalog::new(&providers);
    metadata.output_type = serde_json::to_value(&template)
        .ok()
        .and_then(|value| serde_json::from_value::<WorkflowNode>(value).ok())
        .and_then(|node| spec.output_type(&node, &actions).ok().flatten());
    metadata
}

// -- trigger catalog ---------------------------------------------------------------------------

/// ordered UI metadata for every workflow trigger kind.
pub fn trigger_kind_catalog() -> Vec<WorkflowTriggerKindMetadata> {
    WorkflowTriggerKind::ALL
        .iter()
        .map(|kind| trigger_kind_metadata(kind.clone()))
        .collect()
}

fn trigger_kind_metadata(kind: WorkflowTriggerKind) -> WorkflowTriggerKindMetadata {
    match kind {
        WorkflowTriggerKind::Cron => WorkflowTriggerKindMetadata {
            kind,
            label: "Cron".to_string(),
            icon: "clock".to_string(),
            description: "Fires on a cron schedule.".to_string(),
            fields: vec![
                UiField::new(
                    ParameterMetadata::required("cron", RuninatorType::String)
                        .with_description("Cron expression, e.g. `0 * * * *`."),
                )
                .with_widget("cron"),
                UiField::new(
                    ParameterMetadata::optional("catchup", RuninatorType::String).with_description(
                        "What happens to slots missed while nothing was firing them: `fire_once` (default, collapse the backlog into one run), `fire_all` (replay each), or `skip` (abandon them).",
                    ),
                ),
            ],
            default_configuration: json!({ "cron": "0 * * * *", "parameters": {} }),
        },
        WorkflowTriggerKind::Manual => WorkflowTriggerKindMetadata {
            kind,
            label: "Manual".to_string(),
            icon: "play".to_string(),
            description: "Fired on demand by a user or API call.".to_string(),
            fields: Vec::new(),
            default_configuration: json!({}),
        },
        WorkflowTriggerKind::Chained => WorkflowTriggerKindMetadata {
            kind,
            label: "Chained".to_string(),
            icon: "link".to_string(),
            description: "Starts a target workflow when this workflow run reaches a terminal state."
                .to_string(),
            fields: vec![
                UiField::new(
                    ParameterMetadata::required("target_workflow", RuninatorType::String)
                        .with_description("Name of the workflow to start on completion."),
                )
                .with_widget("workflow_name"),
                UiField::new(
                    ParameterMetadata::required("on", RuninatorType::String).with_description(
                        "Which terminal state fires the chain: `success`, `failure`, or `complete`.",
                    ),
                ),
            ],
            default_configuration: json!({ "on": "success", "target_workflow": "", "parameters": {} }),
        },
    }
}

// -- companion enum catalogs -------------------------------------------------------------------

/// small closed enums the frontend renders as `<select>` controls.
pub fn enum_catalogs() -> Vec<EnumCatalogMetadata> {
    vec![
        EnumCatalogMetadata::new(
            "gate_kind",
            vec![
                EnumOptionMetadata::new("manual", "Manual")
                    .with_description("Opens when an operator releases the gate."),
                EnumOptionMetadata::new("condition", "Condition")
                    .with_description("Opens when a boolean expression becomes true."),
                EnumOptionMetadata::new("external", "External")
                    .with_description("Opens when an external system marks it open."),
            ],
        ),
        EnumCatalogMetadata::new(
            "match_kind",
            vec![
                EnumOptionMetadata::new("equals", "Equals"),
                EnumOptionMetadata::new("not_equals", "Not equals"),
                EnumOptionMetadata::new("exists", "Exists"),
                EnumOptionMetadata::new("when", "When (expression)"),
            ],
        ),
        EnumCatalogMetadata::new(
            "branch_policy",
            vec![
                EnumOptionMetadata::new("all", "All"),
                EnumOptionMetadata::new("any", "Any"),
                EnumOptionMetadata::new("first_success", "First success"),
            ],
        ),
        EnumCatalogMetadata::new(
            "setting_kind",
            vec![
                EnumOptionMetadata::new("config", "Config"),
                EnumOptionMetadata::new("secret", "Secret"),
            ],
        ),
        EnumCatalogMetadata::new("interrupt_source", interrupt_source_options()),
        EnumCatalogMetadata::new("resume_mode", resume_mode_options()),
        EnumCatalogMetadata::new("concurrency_policy", concurrency_policy_options()),
    ]
}

/// what may raise an interrupt, for the header editor's source picker.
///
/// derived from [`InterruptSource::ALL`] rather than listed again, so a new source reaches the UI
/// with the rest of the runtime. the match is exhaustive, which is what makes that automatic.
fn interrupt_source_options() -> Vec<EnumOptionMetadata> {
    InterruptSource::ALL
        .into_iter()
        .map(|source| {
            let (label, description) = match source {
                InterruptSource::External => (
                    "External",
                    "Requested through POST /workflow_runs/{id}/interrupts.",
                ),
                InterruptSource::OrphanSignal => (
                    "Orphan signal",
                    "A signal arrived that no node in the run was parked on.",
                ),
                InterruptSource::Timer => (
                    "Timer",
                    "A workflow-owned periodic timer elapsed. Timer handlers declare their own interval and may be repeated.",
                ),
                InterruptSource::Wake => (
                    "Wake",
                    "A parked cursor's timer elapsed, bound to a wait node's deadline.",
                ),
                InterruptSource::Timeout => (
                    "Timeout",
                    "The node's deadline is about to blow while its run is still in flight, so the handler runs before it gives up.",
                ),
                InterruptSource::Retry => (
                    "Retry",
                    "A failed node run is about to be re-dispatched by the retry policy.",
                ),
                InterruptSource::Failure => (
                    "Failure",
                    "A node run settled failed and the thread is about to take its failure route.",
                ),
                InterruptSource::Resolved => (
                    "Resolved",
                    "An out-of-band park resolution landed: a signal delivered, an approval decided, an input submitted.",
                ),
                InterruptSource::Child => (
                    "Child",
                    "A child run a subflow node is parked on reached a terminal.",
                ),
            };
            EnumOptionMetadata::new(source.as_str(), label).with_description(description)
        })
        .collect()
}

/// what a handler's `resume` node does to the thread it interrupted.
fn resume_mode_options() -> Vec<EnumOptionMetadata> {
    InterruptMode::ALL
        .into_iter()
        .map(|mode| {
            let (label, description) = match mode {
                InterruptMode::Resume => (
                    "Resume",
                    "Resume at the same node and let its handler re-read the node run.",
                ),
                InterruptMode::Continue => (
                    "Continue",
                    "Settle the interrupted node succeeded and take its success edge.",
                ),
                InterruptMode::Restart => (
                    "Restart",
                    "Cancel the in-flight node run and re-enter the node fresh, resetting a park's window.",
                ),
                InterruptMode::Fail => (
                    "Fail",
                    "Settle the interrupted node failed and take its on_failure edge. This does not itself fail the run.",
                ),
            };
            EnumOptionMetadata::new(mode.as_str(), label).with_description(description)
        })
        .collect()
}

/// what the trigger loop does when a workflow is already at its concurrency limit.
fn concurrency_policy_options() -> Vec<EnumOptionMetadata> {
    ConcurrencyPolicy::ALL
        .into_iter()
        .map(|policy| {
            let (label, description) = match policy {
                ConcurrencyPolicy::Allow => (
                    "Allow",
                    "Start the run anyway; overlapping runs are the workflow's problem.",
                ),
                ConcurrencyPolicy::Skip => (
                    "Skip",
                    "Drop the slot: record the firing so it is never retried, and advance to the next one.",
                ),
                ConcurrencyPolicy::Queue => (
                    "Queue",
                    "Leave the slot due and re-evaluate on the next tick, so it fires once capacity frees up.",
                ),
                ConcurrencyPolicy::CancelPrevious => (
                    "Cancel previous",
                    "Cancel the workflow's in-flight runs, then start this one.",
                ),
            };
            EnumOptionMetadata::new(policy.as_str(), label).with_description(description)
        })
        .collect()
}
