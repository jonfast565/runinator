//! nodes that fan work out across branches and gather it back.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use super::builders::{base, control, end_ref, enum_ty, field, opt};
use super::{GraphRole, NodeKindSpec, TargetSlot};
use crate::errors::WorkflowValidationError;
use crate::parameters::{
    parse_join_parameters, parse_map_parameters, parse_parallel_parameters, parse_race_parameters,
};

pub(super) struct Parallel;
pub(super) struct Join;
pub(super) struct Map;
pub(super) struct Race;

impl NodeKindSpec for Parallel {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Parallel
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_parallel_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(parse_parallel_parameters(node)?
            .branches
            .into_iter()
            .map(|branch| TargetSlot::runnable("branches", "parallel branch", branch))
            .collect())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![control("branches", "Parallel branch", &["branches"], true)],
            default_template: json!({
                "kind": "parallel", "parameters": { "branches": [] },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Parallel",
                "parallel",
                "concurrency",
                "Fans out into branches that run concurrently.",
            )
        }
    }
}

impl NodeKindSpec for Join {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Join
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_join_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(parse_join_parameters(node)?
            .wait_for
            .into_iter()
            .map(|target| TargetSlot::runnable("wait_for", "join wait_for", target))
            .collect())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                opt("mode", enum_ty(&["all", "any", "first_success"])),
                FieldLocation::parameters(&["mode"]),
                None,
            )],
            edge_slots: vec![control("wait_for", "Join dependency", &["wait_for"], true)],
            default_template: json!({
                "kind": "join", "parameters": { "wait_for": [], "mode": "all" },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Join",
                "join",
                "concurrency",
                "Waits for upstream branches to finish before continuing.",
            )
        }
    }
}

impl NodeKindSpec for Map {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Map
    }

    fn graph_role(&self) -> GraphRole {
        // the body routes back here once per item, and the simulator does not model fan-out.
        GraphRole::STEP.reentrant().not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_map_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(vec![TargetSlot::runnable(
            "target",
            "map target",
            parse_map_parameters(node)?.target,
        )])
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![
                field(
                    opt("items", RuninatorType::array(RuninatorType::Any)),
                    FieldLocation::parameters(&["items"]),
                    Some("expression"),
                ),
                field(
                    opt("concurrency", RuninatorType::Integer),
                    FieldLocation::parameters(&["concurrency"]),
                    None,
                ),
            ],
            edge_slots: vec![control("target", "Map target", &["target"], false)],
            default_template: json!({
                "kind": "map",
                "parameters": { "items": [], "target": end_ref(), "concurrency": 1 },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Map",
                "grid",
                "concurrency",
                "Runs its target once for each item in a collection.",
            )
        }
    }
}

impl NodeKindSpec for Race {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Race
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP.reentrant().not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_race_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        Ok(parse_race_parameters(node)?
            .branches
            .into_iter()
            .map(|branch| TargetSlot::runnable("branches", "race branch", branch))
            .collect())
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                opt("winner", enum_ty(&["all", "any", "first_success"])),
                FieldLocation::parameters(&["winner"]),
                None,
            )],
            edge_slots: vec![control("branches", "Race branch", &["branches"], true)],
            default_template: json!({
                "kind": "race", "parameters": { "branches": [] },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Race",
                "race",
                "concurrency",
                "Runs branches concurrently; the first to finish wins.",
            )
        }
    }
}
