//! the crate's behaviour tests, partitioned by subject.
//!
//! per-module suites that pair with one source file keep living beside it as `<module>_tests.rs`
//! (`compute_tests`, `functions_tests`, …). what lands here is the cross-module behaviour that has
//! no single owning file: validating a whole definition, typing expressions against provider
//! metadata, and the catalog the ui is driven from.
//!
//! shared setup lives in this file so a submodule's `use super::*` picks it up along with the crate
//! surface, exactly as the single-file suite did.

mod catalog;
mod concurrency;
mod control_flow;
mod interrupts;
mod provider_parameters;
mod references;
mod state_machine;
mod typing;
mod validation;

use crate::*;
use runinator_models::{
    catalog_metadata::LocationBase,
    providers::{
        ActionMetadata, ParameterMetadata, ProviderMetadata, ProviderRuntimeMetadata,
        ResultMetadata, RuninatorType,
    },
    types::RuninatorField,
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowNode, WorkflowNodeKind, WorkflowStatus,
        WorkflowTriggerKind,
    },
};
use std::collections::HashMap;
use uuid::Uuid;

fn workflow(definition: runinator_models::value::Value) -> WorkflowDefinition {
    WorkflowDefinition {
        id: Some(Uuid::now_v7()),
        name: "test".into(),
        namespace: None,
        org_id: None,
        version: runinator_models::semver::SemVer::new(1, 0, 0),
        enabled: true,
        input_type: RuninatorType::Any,
        definition: WorkflowGraph::from_value(definition).unwrap(),
        created_at: None,
        updated_at: None,
    }
}

/// a start → action → end graph whose single action carries `configuration`, so a test only has to
/// name the configuration under test.
fn action_workflow(configuration: runinator_models::value::Value) -> WorkflowDefinition {
    workflow(runinator_models::json!({
        "start": "start",
        "nodes": [
            { "id": "start", "kind": "start", "transitions": { "next": { "$node": "check" } } },
            {
                "id": "check",
                "kind": "action",
                "action": {
                    "provider": "typed",
                    "function": "check",
                    "configuration": configuration
                },
                "transitions": { "on_success": { "$node": "done" } }
            },
            { "id": "done", "kind": "end" }
        ]
    }))
}

/// the provider [`action_workflow`] targets, with its one required parameter typed as asked.
fn check_provider(param_type: RuninatorType) -> ProviderMetadata {
    ProviderMetadata {
        name: "typed".into(),
        actions: vec![
            ActionMetadata::new("check", "check typed input")
                .with_parameters(vec![ParameterMetadata::required("config", param_type)]),
        ],
        metadata: ProviderRuntimeMetadata::default(),
    }
}
