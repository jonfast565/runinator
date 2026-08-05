//! ui metadata catalogs for workflow node kinds, edges, and triggers.
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
use runinator_models::json;
use runinator_models::providers::{ParameterMetadata, RuninatorType};
use runinator_models::workflows::{WorkflowNodeKind, WorkflowTriggerKind};

use crate::node_kinds::spec_for;

// -- the node catalog --------------------------------------------------------------------------

/// ordered ui metadata for every workflow node kind.
pub fn node_kind_catalog() -> Vec<WorkflowNodeKindMetadata> {
    WorkflowNodeKind::ALL.iter().map(node_metadata).collect()
}

/// the palette entry for one node kind.
pub fn node_metadata(kind: &WorkflowNodeKind) -> WorkflowNodeKindMetadata {
    spec_for(kind).metadata()
}

// -- trigger catalog ---------------------------------------------------------------------------

/// ordered ui metadata for every workflow trigger kind.
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
    ]
}
