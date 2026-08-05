//! nodes that decide where the run goes next, or hold it until something says it may continue.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};

use super::builders::{base, condition_branches, control, end_ref, enum_ty, field, opt, req};
use super::{GraphRole, NodeKindSpec, TargetSlot};
use crate::conditions::validate_condition;
use crate::errors::WorkflowValidationError;
use crate::parameters::{
    parse_percentage_parameters, parse_switch_parameters, parse_toggle_parameters,
    parse_try_parameters,
};

pub(super) struct Wait;
pub(super) struct Condition;
pub(super) struct Switch;
pub(super) struct Toggle;
pub(super) struct Percentage;
pub(super) struct Approval;
pub(super) struct Gate;
pub(super) struct Signal;
pub(super) struct Loop;
pub(super) struct Try;
pub(super) struct Assert;
pub(super) struct Checkpoint;

impl NodeKindSpec for Wait {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Wait
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("seconds", RuninatorType::Duration),
                    FieldLocation::wait(&["seconds"]),
                    Some("duration"),
                ),
                field(
                    opt("initial_status", RuninatorType::String),
                    FieldLocation::wait(&["initial_status"]),
                    None,
                ),
                field(
                    opt("until_status", RuninatorType::String),
                    FieldLocation::wait(&["until_status"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "wait", "wait": { "seconds": 60 },
                "parameters": {}, "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Wait",
                "clock",
                "control-flow",
                "Pauses the run for a fixed delay or until a time.",
            )
        }
    }
}

impl NodeKindSpec for Condition {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Condition
    }

    fn graph_role(&self) -> GraphRole {
        // routes by predicate; the decision is the edge taken, not an addressable output.
        GraphRole::STEP.without_output()
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![condition_branches()],
            default_template: json!({
                "kind": "condition", "condition": {},
                "transitions": {
                    "branches": [ { "when": { "value": { "$ref": { "params": ["approved"] } }, "equals": true }, "target": end_ref() } ],
                    "next": end_ref(),
                },
                "parameters": {}, "retry": { "max_attempts": 1 },
            }),
            ..base(
                self,
                "Condition",
                "branch",
                "control-flow",
                "Routes down a branch based on a boolean expression.",
            )
        }
    }
}

impl NodeKindSpec for Switch {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Switch
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        for case in parse_switch_parameters(node)?.cases {
            validate_condition(&case.condition.to_value())?;
        }
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_switch_parameters(node)?;
        let mut slots: Vec<TargetSlot> = params
            .cases
            .into_iter()
            .map(|case| TargetSlot::non_start("cases", "switch case target", case.target))
            .collect();
        slots.extend(
            params
                .default
                .map(|target| TargetSlot::non_start("default", "switch default target", target)),
        );
        Ok(slots)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                req("value", RuninatorType::Any),
                FieldLocation::parameters(&["value"]),
                Some("expression"),
            )],
            edge_slots: vec![
                control("cases", "Switch case", &["cases"], true),
                control("default", "Switch default", &["default"], false),
            ],
            default_template: json!({
                "kind": "switch",
                "parameters": { "value": { "$ref": { "params": ["mode"] } }, "cases": [], "default": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Switch",
                "switch",
                "control-flow",
                "Routes to one of several cases by matching a value.",
            )
        }
    }
}

impl NodeKindSpec for Toggle {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Toggle
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_toggle_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_toggle_parameters(node)?;
        Ok(vec![
            TargetSlot::non_start("on", "toggle on/off target", params.on),
            TargetSlot::non_start("off", "toggle on/off target", params.off),
        ])
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                req("value", RuninatorType::Any),
                FieldLocation::parameters(&["value"]),
                Some("expression"),
            )],
            edge_slots: vec![
                control("on", "Toggle on", &["on"], false),
                control("off", "Toggle off", &["off"], false),
            ],
            default_template: json!({
                "kind": "toggle",
                "parameters": { "value": { "$ref": { "config": ["flags", "enabled"] } }, "on": end_ref(), "off": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Toggle",
                "toggle",
                "control-flow",
                "A light switch: routes to on or off by a value's truthiness.",
            )
        }
    }
}

impl NodeKindSpec for Percentage {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Percentage
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_percentage_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_percentage_parameters(node)?;
        let mut slots: Vec<TargetSlot> = params
            .buckets
            .into_iter()
            .map(|bucket| {
                TargetSlot::non_start("buckets", "percentage bucket target", bucket.target)
            })
            .collect();
        slots.extend(
            params
                .default
                .map(|target| TargetSlot::non_start("default", "percentage bucket target", target)),
        );
        Ok(slots)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![field(
                req("key", RuninatorType::Any),
                FieldLocation::parameters(&["key"]),
                Some("expression"),
            )],
            edge_slots: vec![
                control("buckets", "Bucket", &["buckets"], true),
                control("default", "Percentage default", &["default"], false),
            ],
            default_template: json!({
                "kind": "percentage",
                "parameters": { "key": { "$ref": { "input": ["user_id"] } }, "buckets": [], "default": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Percentage",
                "percentage",
                "control-flow",
                "Weighted rollout: routes to a bucket by a stable hash of a key.",
            )
        }
    }
}

impl NodeKindSpec for Approval {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Approval
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("approval_type", RuninatorType::String),
                    FieldLocation::parameters(&["approval_type"]),
                    None,
                ),
                field(
                    opt("prompt", RuninatorType::String),
                    FieldLocation::parameters(&["prompt"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "approval",
                "parameters": { "approval_type": "generic", "prompt": "Approval required" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_reject": end_ref() },
            }),
            ..base(
                self,
                "Approval",
                "approve",
                "control-flow",
                "Halts until a human approves or rejects.",
            )
        }
    }
}

impl NodeKindSpec for Gate {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Gate
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("kind", enum_ty(&["manual", "condition", "external"])),
                    FieldLocation::parameters(&["kind"]),
                    None,
                ),
                field(
                    opt("when", RuninatorType::Any),
                    FieldLocation::parameters(&["when"]),
                    Some("json"),
                ),
                field(
                    opt("poll_interval", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval"]),
                    None,
                ),
                field(
                    opt("timeout", RuninatorType::Integer),
                    FieldLocation::parameters(&["timeout"]),
                    None,
                ),
                field(
                    opt("label", RuninatorType::String),
                    FieldLocation::parameters(&["label"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "gate",
                "parameters": { "kind": "manual", "poll_interval": 30 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Gate",
                "shield",
                "control-flow",
                "Blocks until an automated/policy check or manual gate opens.",
            )
        }
    }
}

impl NodeKindSpec for Signal {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Signal
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                req("name", RuninatorType::String),
                FieldLocation::parameters(&["name"]),
                None,
            )],
            default_template: json!({
                "kind": "signal",
                "parameters": { "name": "signal" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Signal",
                "bell",
                "control-flow",
                "Pauses until a named external signal is delivered to the run.",
            )
        }
    }
}

impl NodeKindSpec for Loop {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Loop
    }

    fn graph_role(&self) -> GraphRole {
        // the body edge returns here every iteration, and the simulator does not model iteration.
        GraphRole::STEP.reentrant().not_simulatable()
    }

    // no target slots: a loop's body is `transitions.next` and its exit is `transitions.on_success`.
    // the reducer never reads a `parameters.target`, and the wdl lowering never emits one.

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
                    opt("max_iterations", RuninatorType::Integer),
                    FieldLocation::top_level("max_iterations"),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "loop",
                "parameters": { "items": [] },
                "max_iterations": 10,
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Loop",
                "loop",
                "control-flow",
                "Repeats its target node while a condition holds.",
            )
        }
    }
}

impl NodeKindSpec for Try {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Try
    }

    fn graph_role(&self) -> GraphRole {
        // the guarded body routes back here to settle, and the simulator does not model the frame.
        GraphRole::STEP
            .without_output()
            .reentrant()
            .not_simulatable()
    }

    fn check_parameters(&self, node: &WorkflowNode) -> Result<(), WorkflowValidationError> {
        parse_try_parameters(node)?;
        Ok(())
    }

    fn target_slots(
        &self,
        node: &WorkflowNode,
    ) -> Result<Vec<TargetSlot>, WorkflowValidationError> {
        let params = parse_try_parameters(node)?;
        let mut slots = vec![TargetSlot::runnable("body", "try body", params.body)];
        slots.extend(
            params
                .catch
                .map(|target| TargetSlot::runnable("catch", "try catch", target)),
        );
        slots.extend(
            params
                .finally
                .map(|target| TargetSlot::runnable("finally", "try finally", target)),
        );
        Ok(slots)
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![
                control("body", "Try body", &["body"], false),
                control("catch", "Try catch", &["catch"], false),
                control("finally", "Try finally", &["finally"], false),
            ],
            default_template: json!({
                "kind": "try",
                "parameters": { "body": end_ref(), "catch": end_ref(), "finally": end_ref() },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                self,
                "Try",
                "shield",
                "control-flow",
                "Guards a body node and catches failures with a handler.",
            )
        }
    }
}

impl NodeKindSpec for Assert {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Assert
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                opt(
                    "assertions",
                    RuninatorType::Array(Box::new(RuninatorType::Any)),
                ),
                FieldLocation::parameters(&["assertions"]),
                Some("assertions"),
            )],
            default_template: json!({
                "kind": "assert", "parameters": { "assertions": [] },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Assert",
                "check",
                "control-flow",
                "Evaluates named boolean assertions; fails with a structured violation list.",
            )
        }
    }
}

impl NodeKindSpec for Checkpoint {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Checkpoint
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![field(
                req("name", RuninatorType::String),
                FieldLocation::parameters(&["name"]),
                None,
            )],
            default_template: json!({
                "kind": "checkpoint", "parameters": { "name": "checkpoint" },
                "retry": { "max_attempts": 1 }, "transitions": { "next": end_ref() },
            }),
            ..base(
                self,
                "Checkpoint",
                "save",
                "control-flow",
                "Snapshots run state at a named point; enables rollback via the control-plane API.",
            )
        }
    }
}
