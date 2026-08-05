//! nodes that coordinate across runs: locks, rate limits, and cross-run rendezvous.

use runinator_models::catalog_metadata::{FieldLocation, WorkflowNodeKindMetadata};
use runinator_models::json;
use runinator_models::providers::RuninatorType;
use runinator_models::workflows::WorkflowNodeKind;

use super::builders::{base, end_ref, enum_ty, field, opt, req};
use super::{GraphRole, NodeKindSpec};

pub(super) struct Mutex;
pub(super) struct Throttle;
pub(super) struct Cooldown;
pub(super) struct AwaitRun;
pub(super) struct Debounce;
pub(super) struct Collect;
pub(super) struct Barrier;
pub(super) struct CircuitBreaker;

impl NodeKindSpec for Mutex {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Mutex
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
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
                self,
                "Mutex",
                "lock",
                "sync",
                "Acquires a named distributed mutex, held until the run ends or a matching release node.",
            )
        }
    }
}

impl NodeKindSpec for Throttle {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Throttle
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
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
                self,
                "Throttle",
                "hourglass",
                "sync",
                "Enforces a cross-run rate limit; parks until a token is available.",
            )
        }
    }
}

impl NodeKindSpec for Cooldown {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Cooldown
    }

    fn graph_role(&self) -> GraphRole {
        // short-circuits the run rather than recording a value downstream nodes read.
        GraphRole::STEP.without_output()
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("name", RuninatorType::String),
                    FieldLocation::parameters(&["name"]),
                    None,
                ),
                field(
                    opt("window_seconds", RuninatorType::Integer),
                    FieldLocation::parameters(&["window_seconds"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "cooldown",
                "parameters": { "name": "my-cooldown", "window_seconds": 900 },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref() },
            }),
            ..base(
                self,
                "Cooldown",
                "hourglass",
                "sync",
                "Short-circuits the run to success when a prior pass ran within the window; at most one pass proceeds per window.",
            )
        }
    }
}

impl NodeKindSpec for AwaitRun {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::AwaitRun
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
            fields: vec![
                field(
                    req("workflow", RuninatorType::String),
                    FieldLocation::parameters(&["workflow"]),
                    None,
                ),
                field(
                    opt("key", RuninatorType::Any),
                    FieldLocation::parameters(&["key"]),
                    Some("expression"),
                ),
                field(
                    opt("mode", enum_ty(&["all", "any"])),
                    FieldLocation::parameters(&["mode"]),
                    None,
                ),
            ],
            default_template: json!({
                "kind": "await_run", "parameters": { "workflow": "", "mode": "all" },
                "retry": { "max_attempts": 1 },
                "transitions": { "on_success": end_ref(), "on_failure": end_ref() },
            }),
            ..base(
                self,
                "Await Workflow",
                "runs",
                "sync",
                "Pauses until run(s) of a named workflow (optionally matching a correlation key) reach a terminal state.",
            )
        }
    }
}

impl NodeKindSpec for Debounce {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Debounce
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
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
                self,
                "Debounce",
                "clock",
                "sync",
                "Parks with a trailing delay that resets on re-trigger; collapses event bursts.",
            )
        }
    }
}

impl NodeKindSpec for Collect {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Collect
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
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
                self,
                "Collect",
                "list",
                "sync",
                "Accumulates externally-delivered items until a count or time threshold is met.",
            )
        }
    }
}

impl NodeKindSpec for Barrier {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::Barrier
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
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
                self,
                "Barrier",
                "join",
                "sync",
                "Parks until N runs reach this named barrier; the last arrival releases all waiters.",
            )
        }
    }
}

impl NodeKindSpec for CircuitBreaker {
    fn kind(&self) -> WorkflowNodeKind {
        WorkflowNodeKind::CircuitBreaker
    }

    fn graph_role(&self) -> GraphRole {
        GraphRole::STEP
    }

    fn metadata(&self) -> WorkflowNodeKindMetadata {
        WorkflowNodeKindMetadata {
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
                self,
                "Circuit Breaker",
                "shield",
                "sync",
                "Tracks failure rates across runs; fast-fails or routes to fallback when tripped.",
            )
        }
    }
}
