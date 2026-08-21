use std::collections::HashMap;

use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::SendableError;
use runinator_models::value::Value;
use runinator_models::workflows::WorkflowDefinition;
use runinator_workflows::{
    NodeEvalRequest, NodeOutcome, SimulationEnv, SimulationRun, simulate_workflow,
};
use uuid::Uuid;

/// the database-backed implementation of `SimulationEnv`: config comes from the live settings store,
/// and task/park outcomes replay a prior run's durable VM effect receipts. Nodes with no recorded
/// output default to success, so a dry-run of a never-executed workflow still walks its control flow.
///
/// This is the production counterpart to the mock env used by `runinatorctl workflows test`: the same
/// walker drives both, so a dry-run against real config/history and an offline unit test agree.
pub struct DbSimulationEnv {
    config: Value,
    recorded: HashMap<String, NodeOutcome>,
}

impl DbSimulationEnv {
    /// load config from the settings store and, when `replay_run` is set, that run's recorded
    /// effect outcomes. Async because both come from the database; the trait methods are then pure
    /// reads.
    pub async fn load<T: DatabaseImpl>(db: &T, replay_run: Option<Uuid>) -> Self {
        let config = runinator_runtime::config::config_tree(db).await;
        let mut recorded = HashMap::new();
        if let Some(run_id) = replay_run
            && let (Ok(Some(module)), Ok(effects)) = (
                db.fetch_workflow_module(run_id).await,
                db.fetch_workflow_effects(run_id).await,
            )
        {
            for effect in effects {
                let Ok(Some(continuation)) =
                    db.fetch_workflow_continuation(effect.continuation_id).await
                else {
                    continue;
                };
                let Some(location) =
                    module.graph_location(continuation.instruction_pointer.saturating_sub(1))
                else {
                    continue;
                };
                // Later visits (for example loop iterations) overwrite earlier ones; the latest
                // durable effect receipt is the replay outcome for that source node.
                recorded.insert(
                    location.node_id.clone(),
                    NodeOutcome {
                        status: effect.status.workflow_status(),
                        output: effect.result.unwrap_or(Value::Null),
                    },
                );
            }
        }
        Self { config, recorded }
    }

    fn outcome_for(&self, node_id: &str) -> NodeOutcome {
        self.recorded
            .get(node_id)
            .cloned()
            .unwrap_or_else(|| NodeOutcome::succeeded(Value::Null))
    }
}

impl SimulationEnv for DbSimulationEnv {
    fn config_tree(&mut self) -> Value {
        self.config.clone()
    }

    fn evaluate_action(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcome_for(&request.node.id)
    }

    fn resolve_park(&mut self, request: &NodeEvalRequest<'_>) -> NodeOutcome {
        self.outcome_for(&request.node.id)
    }
}

/// dry-run `workflow` against live config (and optionally a prior run's outputs) without publishing
/// effects. Used for server-side branch preview and live editing.
pub async fn simulate_run<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
    inputs: Value,
    replay_run: Option<Uuid>,
) -> Result<SimulationRun, SendableError> {
    let mut env = DbSimulationEnv::load(db, replay_run).await;
    simulate_workflow(workflow, inputs, &mut env).map_err(|err| -> SendableError { Box::new(err) })
}
