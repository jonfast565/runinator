//! nodes that move values in and out of the run without dispatching work.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::types::RuninatorType as WorkflowType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use super::builders::{base, end_ref, field, opt};
use super::{ActionCatalog, GraphRole, NodeKindSpec};
use crate::errors::WorkflowValidationError;
use crate::parameters::{parse_input_parameters, parse_output_parameters};

pub(super) struct Output;
pub(super) struct Input;
pub(super) struct Config;
pub(super) struct Transform;
pub(super) struct Audit;
pub(super) struct EventSource;

impl NodeKindSpec for Output {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Output
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_output_parameters(node)?;
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::structure([
            ("event_type", WorkflowType::String),
            ("data", WorkflowType::Any),
            ("artifacts", WorkflowType::Any),
        ])))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("event_type", RuninatorType::String),
                    FieldLocation::parameters(&["event_type"]),
                    None,
                ),
                field(
                    opt("data", RuninatorType::Any),
                    FieldLocation::parameters(&["data"]),
                    Some("json"),
                ),
            ],
            default_template: json!({
                "kind": "output",
                "parameters": { "event_type": "workflow.output", "data": {} },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Output",
                "output",
                "io",
                "Publishes output without interrupting the flow.",
            )
        }
    }
}

impl NodeKindSpec for Input {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Input
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        let _ = parse_input_parameters(node);
        Ok(())
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::Any))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                opt("prompt", RuninatorType::String),
                FieldLocation::parameters(&["prompt"]),
                None,
            )],
            default_template: json!({
                "kind": "input", "parameters": { "prompt": "Provide input" },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Input",
                "message",
                "io",
                "Waits for a user-supplied value from the UI.",
            )
        }
    }
}

impl NodeKindSpec for Config {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Config
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn output_type(
        &self,
        _node: &WorkflowNode,
        _actions: &ActionCatalog<'_>,
    ) -> Result<Option<WorkflowType>, WorkflowValidationError> {
        Ok(Some(WorkflowType::structure([
            ("name", WorkflowType::String),
            ("metadata", WorkflowType::Any),
        ])))
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("name", RuninatorType::Any),
                    FieldLocation::parameters(&["name"]),
                    Some("json"),
                ),
                field(
                    opt("metadata", RuninatorType::Any),
                    FieldLocation::parameters(&["metadata"]),
                    Some("json"),
                ),
            ],
            default_template: json!({
                "kind": "config", "parameters": { "name": "", "metadata": {} },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Config",
                "gear",
                "io",
                "Sets configuration values for downstream nodes.",
            )
        }
    }
}

impl NodeKindSpec for Transform {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Transform
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                opt("bindings", RuninatorType::Any),
                FieldLocation::parameters(&["bindings"]),
                Some("json"),
            )],
            default_template: json!({
                "kind": "transform", "parameters": { "bindings": {} },
                "retry": { "max_attempts": 1 }, "transitions": { "next": end_ref() },
            }),
            ..base(
                self,
                "Transform",
                "gear",
                "io",
                "Resolves named expression bindings into the run context; no side effects.",
            )
        }
    }
}

impl NodeKindSpec for Audit {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Audit
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("action", RuninatorType::Any),
                    FieldLocation::parameters(&["action"]),
                    Some("expression"),
                ),
                field(
                    opt("actor", RuninatorType::Any),
                    FieldLocation::parameters(&["actor"]),
                    Some("expression"),
                ),
                field(
                    opt("target", RuninatorType::Any),
                    FieldLocation::parameters(&["target"]),
                    Some("expression"),
                ),
                field(
                    opt("reason", RuninatorType::Any),
                    FieldLocation::parameters(&["reason"]),
                    Some("expression"),
                ),
            ],
            default_template: json!({
                "kind": "audit", "parameters": { "action": "" },
                "retry": { "max_attempts": 1 }, "transitions": { "next": end_ref() },
            }),
            ..base(
                self,
                "Audit",
                "file",
                "io",
                "Appends a tamper-evident audit record to the workflow log.",
            )
        }
    }
}

impl NodeKindSpec for EventSource {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::EventSource
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("event_type", RuninatorType::String),
                    FieldLocation::parameters(&["event_type"]),
                    None,
                ),
                field(
                    opt("filter", RuninatorType::Any),
                    FieldLocation::parameters(&["filter"]),
                    Some("expression"),
                ),
                field(
                    opt("max", RuninatorType::Integer),
                    FieldLocation::parameters(&["max"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "event_source", "parameters": { "event_type": "" },
                "retry": { "max_attempts": 1 }, "transitions": { "on_success": end_ref() },
            }),
            ..base(
                self,
                "Event Source",
                "bell",
                "io",
                "Subscribes to a named event stream; drives a body subgraph on each matching event.",
            )
        }
    }
}
