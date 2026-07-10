//! ui metadata catalogs for workflow node kinds, edges, and triggers.
//!
//! this is the single source of truth the command center renders from (palette, generic step
//! editor, read-only detail view, edge palette, trigger forms). it lives next to the per-kind
//! parameter parsers (`parameters.rs`) and validation (`validation.rs`) so the field schemas stay
//! aligned with the code that enforces them. adding a node/edge/trigger kind is a change here plus
//! the model enum — the frontend needs no per-kind edits.

use runinator_models::catalog_metadata::{
    EdgeTaxonomy, EnumCatalogMetadata, EnumOptionMetadata, FieldLocation, NodeEdgeSlot,
    NodeFieldMetadata, UiField, WorkflowNodeKindMetadata, WorkflowTriggerKindMetadata,
};
use runinator_models::json;
use runinator_models::providers::{ParameterMetadata, RuninatorType};
use runinator_models::value::Value;
use runinator_models::workflows::{WorkflowNodeKind, WorkflowTriggerKind};

// -- small field/edge builders to keep the catalog readable ------------------------------------

fn req(name: &str, ty: RuninatorType) -> ParameterMetadata {
    ParameterMetadata::required(name, ty)
}

fn opt(name: &str, ty: RuninatorType) -> ParameterMetadata {
    ParameterMetadata::optional(name, ty)
}

fn enum_ty(values: &[&str]) -> RuninatorType {
    RuninatorType::Enum(
        values
            .iter()
            .map(|v| Value::String((*v).to_string()))
            .collect(),
    )
}

/// a field bound to a node-json location, with an optional widget hint.
fn field(
    param: ParameterMetadata,
    location: FieldLocation,
    widget: Option<&str>,
) -> NodeFieldMetadata {
    let ui = match widget {
        Some(widget) => UiField::new(param).with_widget(widget),
        None => UiField::new(param),
    };
    NodeFieldMetadata::new(ui, location)
}

/// a per-kind control-flow edge whose target is stored in the node's parameters.
fn control(key: &str, label: &str, path: &[&str], multiple: bool) -> NodeEdgeSlot {
    NodeEdgeSlot {
        key: key.to_string(),
        label: label.to_string(),
        description: None,
        taxonomy: EdgeTaxonomy::Control,
        target: FieldLocation::parameters(path),
        multiple,
        editable_label: false,
        editable_condition: false,
        orderable: multiple,
    }
}

/// the condition-branch slot: a list of `when -> target` routes in `transitions.branches`.
fn condition_branches() -> NodeEdgeSlot {
    NodeEdgeSlot {
        key: "branches".to_string(),
        label: "Condition branch".to_string(),
        description: Some("A conditional route taken when its predicate matches.".to_string()),
        taxonomy: EdgeTaxonomy::Branch,
        target: FieldLocation::transitions(&["branches"]),
        multiple: true,
        editable_label: true,
        editable_condition: true,
        orderable: true,
    }
}

/// base descriptor for a standard, addable node kind (predicate edges on, not protected/terminal).
fn base(
    kind: WorkflowNodeKind,
    label: &str,
    icon: &str,
    category: &str,
    description: &str,
) -> WorkflowNodeKindMetadata {
    WorkflowNodeKindMetadata {
        kind,
        label: label.to_string(),
        icon: icon.to_string(),
        description: description.to_string(),
        category: category.to_string(),
        protected: false,
        terminal: false,
        addable: true,
        supports_predicate_edges: true,
        fields: Vec::new(),
        edge_slots: Vec::new(),
        default_template: Value::Null,
    }
}

fn end_ref() -> Value {
    json!({ "$node": "end" })
}

// -- the node catalog --------------------------------------------------------------------------

/// ordered ui metadata for every workflow node kind. the `match` is exhaustive, so a new
/// `WorkflowNodeKind` variant fails to compile until it is described here.
pub fn node_kind_catalog() -> Vec<WorkflowNodeKindMetadata> {
    WorkflowNodeKind::ALL
        .iter()
        .map(|kind| node_kind_metadata(kind.clone()))
        .collect()
}

fn node_kind_metadata(kind: WorkflowNodeKind) -> WorkflowNodeKindMetadata {
    match kind {
        WorkflowNodeKind::Start => WorkflowNodeKindMetadata {
            protected: true,
            addable: false,
            supports_predicate_edges: false,
            default_template: json!({ "kind": "start", "transitions": {} }),
            ..base(
                kind,
                "Start",
                "play",
                "terminal",
                "Entry point where the workflow run begins.",
            )
        },
        WorkflowNodeKind::End => WorkflowNodeKindMetadata {
            protected: true,
            terminal: true,
            addable: false,
            supports_predicate_edges: false,
            default_template: json!({ "kind": "end" }),
            ..base(
                kind,
                "End",
                "flag",
                "terminal",
                "Terminal node that completes the run successfully.",
            )
        },
        WorkflowNodeKind::Fail => WorkflowNodeKindMetadata {
            protected: true,
            terminal: true,
            addable: false,
            supports_predicate_edges: false,
            default_template: json!({ "kind": "fail" }),
            ..base(
                kind,
                "Fail",
                "alert",
                "terminal",
                "Terminal node that ends the run as failed.",
            )
        },
        WorkflowNodeKind::Action => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("provider", RuninatorType::String),
                    FieldLocation::action(&["provider"]),
                    Some("provider"),
                ),
                field(
                    req("function", RuninatorType::String),
                    FieldLocation::action(&["function"]),
                    Some("action_function"),
                ),
                field(
                    opt("timeout_seconds", RuninatorType::Integer),
                    FieldLocation::action(&["timeout_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "action",
                "action": { "provider": "", "function": "", "timeout_seconds": 300, "configuration": {} },
                "parameters": {},
                "retry": { "max_attempts": 1 },
                "transitions": {},
            }),
            ..base(
                kind,
                "Action",
                "bolt",
                "task",
                "Runs a task through a provider action.",
            )
        },
        WorkflowNodeKind::Wait => WorkflowNodeKindMetadata {
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
                kind,
                "Wait",
                "clock",
                "control-flow",
                "Pauses the run for a fixed delay or until a time.",
            )
        },
        WorkflowNodeKind::Condition => WorkflowNodeKindMetadata {
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
                kind,
                "Condition",
                "branch",
                "control-flow",
                "Routes down a branch based on a boolean expression.",
            )
        },
        WorkflowNodeKind::Switch => WorkflowNodeKindMetadata {
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
                kind,
                "Switch",
                "switch",
                "control-flow",
                "Routes to one of several cases by matching a value.",
            )
        },
        WorkflowNodeKind::Toggle => WorkflowNodeKindMetadata {
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
                kind,
                "Toggle",
                "toggle",
                "control-flow",
                "A light switch: routes to on or off by a value's truthiness.",
            )
        },
        WorkflowNodeKind::Percentage => WorkflowNodeKindMetadata {
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
                kind,
                "Percentage",
                "percentage",
                "control-flow",
                "Weighted rollout: routes to a bucket by a stable hash of a key.",
            )
        },
        WorkflowNodeKind::Approval => WorkflowNodeKindMetadata {
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
                kind,
                "Approval",
                "approve",
                "control-flow",
                "Halts until a human approves or rejects.",
            )
        },
        WorkflowNodeKind::Gate => WorkflowNodeKindMetadata {
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
                kind,
                "Gate",
                "shield",
                "control-flow",
                "Blocks until an automated/policy check or manual gate opens.",
            )
        },
        WorkflowNodeKind::Signal => WorkflowNodeKindMetadata {
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
                kind,
                "Signal",
                "bell",
                "control-flow",
                "Pauses until a named external signal is delivered to the run.",
            )
        },
        WorkflowNodeKind::Loop => WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![
                field(
                    opt("items", RuninatorType::Any),
                    FieldLocation::parameters(&["items"]),
                    Some("expression"),
                ),
                field(
                    opt("max_iterations", RuninatorType::Integer),
                    FieldLocation::top_level("max_iterations"),
                    None,
                ),
            ],
            edge_slots: vec![control("target", "Loop target", &["target"], false)],
            default_template: json!({
                "kind": "loop",
                "parameters": { "items": [], "target": end_ref() },
                "max_iterations": 10,
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                kind,
                "Loop",
                "loop",
                "control-flow",
                "Repeats its target node while a condition holds.",
            )
        },
        WorkflowNodeKind::Parallel => WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            edge_slots: vec![control("branches", "Parallel branch", &["branches"], true)],
            default_template: json!({
                "kind": "parallel", "parameters": { "branches": [] },
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                kind,
                "Parallel",
                "parallel",
                "concurrency",
                "Fans out into branches that run concurrently.",
            )
        },
        WorkflowNodeKind::Join => WorkflowNodeKindMetadata {
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
                kind,
                "Join",
                "join",
                "concurrency",
                "Waits for upstream branches to finish before continuing.",
            )
        },
        WorkflowNodeKind::Try => WorkflowNodeKindMetadata {
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
                kind,
                "Try",
                "shield",
                "control-flow",
                "Guards a body node and catches failures with a handler.",
            )
        },
        WorkflowNodeKind::Map => WorkflowNodeKindMetadata {
            supports_predicate_edges: false,
            fields: vec![
                field(
                    opt("items", RuninatorType::Any),
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
                kind,
                "Map",
                "grid",
                "concurrency",
                "Runs its target once for each item in a collection.",
            )
        },
        WorkflowNodeKind::Race => WorkflowNodeKindMetadata {
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
                kind,
                "Race",
                "race",
                "concurrency",
                "Runs branches concurrently; the first to finish wins.",
            )
        },
        WorkflowNodeKind::Output => WorkflowNodeKindMetadata {
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
                kind,
                "Output",
                "output",
                "io",
                "Publishes output without interrupting the flow.",
            )
        },
        WorkflowNodeKind::Input => WorkflowNodeKindMetadata {
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
                kind,
                "Input",
                "message",
                "io",
                "Waits for a user-supplied value from the UI.",
            )
        },
        WorkflowNodeKind::Config => WorkflowNodeKindMetadata {
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
                kind,
                "Config",
                "gear",
                "io",
                "Sets configuration values for downstream nodes.",
            )
        },
        WorkflowNodeKind::Subflow => WorkflowNodeKindMetadata {
            fields: vec![field(
                req("subflow_id", RuninatorType::String),
                FieldLocation::top_level("subflow_id"),
                Some("subflow"),
            )],
            default_template: json!({
                "kind": "subflow", "subflow_id": null, "parameters": {},
                "retry": { "max_attempts": 1 }, "transitions": {},
            }),
            ..base(
                kind,
                "Subflow",
                "workflow",
                "task",
                "Invokes another workflow as a nested step.",
            )
        },
        WorkflowNodeKind::Assert => WorkflowNodeKindMetadata {
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
                kind,
                "Assert",
                "check",
                "control-flow",
                "Evaluates named boolean assertions; fails with a structured violation list.",
            )
        },
        WorkflowNodeKind::Transform => WorkflowNodeKindMetadata {
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
                kind,
                "Transform",
                "gear",
                "io",
                "Resolves named expression bindings into the run context; no side effects.",
            )
        },
        WorkflowNodeKind::Audit => WorkflowNodeKindMetadata {
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
                kind,
                "Audit",
                "file",
                "io",
                "Appends a tamper-evident audit record to the workflow log.",
            )
        },
        WorkflowNodeKind::Checkpoint => WorkflowNodeKindMetadata {
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
                kind,
                "Checkpoint",
                "save",
                "control-flow",
                "Snapshots run state at a named point; enables rollback via the control-plane API.",
            )
        },
        WorkflowNodeKind::Mutex => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("poll_interval_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
                field(
                    opt("release", RuninatorType::Boolean),
                    FieldLocation::parameters(&["release"]),
                    None,
                ),
                field(
                    opt("hold_timeout_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["hold_timeout_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "mutex", "parameters": { "name": "my-mutex" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                kind,
                "Mutex",
                "lock",
                "sync",
                "Acquires a named distributed mutex, held until the run ends or a matching release node.",
            )
        },
        WorkflowNodeKind::Throttle => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("max_per_window", RuninatorType::Integer),
                    FieldLocation::parameters(&["max_per_window"]),
                    None,
                ),
                field(
                    opt("window_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["window_seconds"]),
                    None,
                ),
                field(
                    opt("poll_interval_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "throttle",
                "parameters": { "name": "my-throttle", "max_per_window": 10, "window_seconds": 60 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                kind,
                "Throttle",
                "hourglass",
                "sync",
                "Enforces a cross-run rate limit; parks until a token is available.",
            )
        },
        WorkflowNodeKind::AwaitRun => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    opt("run_ids", RuninatorType::Any),
                    FieldLocation::parameters(&["run_ids"]),
                    Some("expression"),
                ),
                field(
                    opt("mode", enum_ty(&["all", "any"])),
                    FieldLocation::parameters(&["mode"]),
                    None,
                ),
                field(
                    opt("poll_interval_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "await_run", "parameters": { "run_ids": [], "mode": "all" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                kind,
                "Await Run",
                "runs",
                "sync",
                "Waits for one or more independently-started runs to reach a terminal state.",
            )
        },
        WorkflowNodeKind::Debounce => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("delay_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["delay_seconds"]),
                    None,
                ),
                field(
                    opt("trigger_key", RuninatorType::Any),
                    FieldLocation::parameters(&["trigger_key"]),
                    Some("expression"),
                ),
            ],
            default_template: json!({
                "kind": "debounce", "parameters": { "name": "my-debounce", "delay_seconds": 30 },
                "retry": { "max_attempts": 1 }, "transitions": { "on_success": end_ref() },
            }),
            ..base(
                kind,
                "Debounce",
                "clock",
                "sync",
                "Parks with a trailing delay that resets on re-trigger; collapses event bursts.",
            )
        },
        WorkflowNodeKind::Collect => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("max", RuninatorType::Integer),
                    FieldLocation::parameters(&["max"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "collect", "parameters": { "name": "my-collect", "max": 10 },
                "retry": { "max_attempts": 1 }, "transitions": { "on_success": end_ref() },
            }),
            ..base(
                kind,
                "Collect",
                "list",
                "sync",
                "Accumulates externally-delivered items until a count or time threshold is met.",
            )
        },
        WorkflowNodeKind::Barrier => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("count", RuninatorType::Integer),
                    FieldLocation::parameters(&["count"]),
                    None,
                ),
                field(
                    opt("poll_interval_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["poll_interval_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "barrier", "parameters": { "name": "my-barrier", "count": 2 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                kind,
                "Barrier",
                "join",
                "sync",
                "Parks until N runs reach this named barrier; the last arrival releases all waiters.",
            )
        },
        WorkflowNodeKind::CircuitBreaker => WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("threshold", RuninatorType::Integer),
                    FieldLocation::parameters(&["threshold"]),
                    None,
                ),
                field(
                    opt("window_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["window_seconds"]),
                    None,
                ),
                field(
                    opt("cooldown_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["cooldown_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "circuit_breaker",
                "parameters": { "name": "my-circuit-breaker", "threshold": 5, "window_seconds": 60, "cooldown_seconds": 30 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                kind,
                "Circuit Breaker",
                "shield",
                "sync",
                "Tracks failure rates across runs; fast-fails or routes to fallback when tripped.",
            )
        },
        WorkflowNodeKind::EventSource => WorkflowNodeKindMetadata {
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
                kind,
                "Event Source",
                "bell",
                "io",
                "Subscribes to a named event stream; drives a body subgraph on each matching event.",
            )
        },
    }
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
