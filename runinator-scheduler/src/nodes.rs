// node execution module root.
//
// each workflow node kind is served by a `NodeHandler` (see `handlers`). the scheduler dispatches a
// node by looking up its handler in the `NodeRegistry` and running it against a `NodeContext`; the
// resulting `NodeOutcome` is fanned out to the registered `NodeLifecycleHook`s. adding a node kind
// means adding a handler and registering it in `handlers::builtins` — no dispatch edits.

mod context;
mod driver;
mod handler;
mod handlers;
mod hooks;
mod registry;
mod run_state;

use std::sync::{Arc, OnceLock};

use runinator_broker::Broker;
use runinator_comm::WireCodec;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    workflow_state::SkippedOutput,
    workflows::{WorkflowNode, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};

use crate::api::WorkflowSchedulerApi;

pub use context::NodeContext;
pub use handler::NodeHandler;
pub use hooks::{NodeLifecycleHook, TracingHook};
pub use registry::NodeRegistry;
// the node-authoring surface: `NodeOutcome` is what a handler returns, `RunState` is the typed
// state interface control-flow handlers build on. re-exported for discoverability even though the
// crate's only current callers reach them by full path.
#[allow(unused_imports)]
pub use handler::NodeOutcome;
#[allow(unused_imports)]
pub use run_state::RunState;

// process-wide singletons. the registry and hook set are immutable after construction, so a single
// shared instance avoids rebuilding them on every scheduling tick.
static REGISTRY: OnceLock<NodeRegistry> = OnceLock::new();
static HOOKS: OnceLock<Vec<Arc<dyn NodeLifecycleHook>>> = OnceLock::new();

fn registry() -> &'static NodeRegistry {
    REGISTRY.get_or_init(NodeRegistry::with_builtins)
}

pub(crate) fn lifecycle_hooks() -> &'static [Arc<dyn NodeLifecycleHook>] {
    HOOKS.get_or_init(|| vec![Arc::new(TracingHook) as Arc<dyn NodeLifecycleHook>])
}

/// run the active node for one scheduling tick: resolve its handler, execute it, and notify hooks.
pub async fn dispatch_node(
    broker: &dyn Broker,
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let ctx = NodeContext::new(api, Some(broker), workflow_run, node, latest, node_runs);
    let Some(handler) = registry().get(&node.kind) else {
        return Err(Box::new(RuntimeError::new(
            "workflow.node.unhandled_kind".into(),
            format!("No handler registered for node kind {:?}", node.kind),
        )));
    };
    let outcome = handler.process(&ctx).await?;
    hooks::fire_outcome(lifecycle_hooks(), &ctx, &outcome).await;
    Ok(())
}

/// notify hooks that the active node was paused (debug or explicit pause request).
pub(crate) async fn fire_paused(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) {
    let ctx = NodeContext::new(api, None, workflow_run, node, latest, node_runs);
    for hook in lifecycle_hooks() {
        hook.on_paused(&ctx).await;
    }
}

/// settle a skipped node immediately as succeeded so the workflow follows its success transition.
pub async fn process_skipped_node(
    api: &dyn WorkflowSchedulerApi,
    workflow_run: &WorkflowRun,
    node: &WorkflowNode,
    latest: Option<&WorkflowNodeRun>,
    node_runs: &[WorkflowNodeRun],
) -> Result<(), SendableError> {
    let ctx = NodeContext::new(api, None, workflow_run, node, latest, node_runs);
    let node_run = ctx.ensure_node_run().await?;
    let output = SkippedOutput {
        skipped: true,
        node_id: node.id.clone(),
    };
    ctx.transition(
        &node_run,
        WorkflowStatus::Succeeded,
        Some(output.to_wire_value()?),
        Some(format!("Node {} skipped", node.id)),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
pub use shims::*;

#[cfg(test)]
pub(crate) use handlers::workflow_task_idempotency_key;

// thin shims that run a single handler against a context, used by unit tests that exercise nodes
// directly without the scheduler loop. they do not fire lifecycle hooks.
#[cfg(test)]
mod shims {
    use super::*;
    use handlers::*;

    macro_rules! run_handler {
        ($handler:expr, $api:expr, $run:expr, $node:expr, $latest:expr, $node_runs:expr) => {{
            let ctx = NodeContext::new($api, None, $run, $node, $latest, $node_runs);
            $handler.process(&ctx).await.map(|_| ())
        }};
    }

    pub async fn process_switch_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(SwitchHandler, api, run, node, None, node_runs)
    }

    pub async fn process_emit_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(EmitHandler, api, run, node, None, node_runs)
    }

    pub async fn process_parallel_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        latest: Option<&WorkflowNodeRun>,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(ParallelHandler, api, run, node, latest, node_runs)
    }

    pub async fn process_join_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        latest: Option<&WorkflowNodeRun>,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(JoinHandler, api, run, node, latest, node_runs)
    }

    pub async fn process_try_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        latest: Option<&WorkflowNodeRun>,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(TryHandler, api, run, node, latest, node_runs)
    }

    pub async fn process_map_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        latest: Option<&WorkflowNodeRun>,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(MapHandler, api, run, node, latest, node_runs)
    }

    pub async fn process_race_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        latest: Option<&WorkflowNodeRun>,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(RaceHandler, api, run, node, latest, node_runs)
    }

    pub async fn process_subflow_node(
        api: &dyn WorkflowSchedulerApi,
        run: &WorkflowRun,
        node: &WorkflowNode,
        latest: Option<&WorkflowNodeRun>,
        node_runs: &[WorkflowNodeRun],
    ) -> Result<(), SendableError> {
        run_handler!(SubflowHandler, api, run, node, latest, node_runs)
    }
}
