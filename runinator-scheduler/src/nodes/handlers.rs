// concrete node handlers, one per `WorkflowNodeKind`.
//
// grouped by execution shape: synchronous nodes that evaluate and move on in a single tick, poller
// nodes that kick off work and re-enter until it settles, and control-flow nodes that drive nested
// node graphs through `RunState` frames.

mod control_flow;
mod pollers;
mod stateless;
mod subflow;

use std::sync::Arc;

use crate::nodes::handler::NodeHandler;

pub use control_flow::{LoopHandler, MapHandler, ParallelHandler, RaceHandler, TryHandler};
pub use pollers::{ActionHandler, ApprovalHandler, JoinHandler, WaitHandler};
pub use subflow::SubflowHandler;

#[cfg(test)]
pub(crate) use pollers::workflow_task_idempotency_key;
pub use stateless::{
    ConditionHandler, ConfigHandler, EmitHandler, EndHandler, FailHandler, StartHandler,
    SwitchHandler,
};

/// every builtin handler, registered once.
pub fn builtins() -> Vec<Arc<dyn NodeHandler>> {
    vec![
        Arc::new(StartHandler),
        Arc::new(ActionHandler),
        Arc::new(WaitHandler),
        Arc::new(ConditionHandler),
        Arc::new(SwitchHandler),
        Arc::new(ApprovalHandler),
        Arc::new(LoopHandler),
        Arc::new(ParallelHandler),
        Arc::new(JoinHandler),
        Arc::new(TryHandler),
        Arc::new(MapHandler),
        Arc::new(RaceHandler),
        Arc::new(EmitHandler),
        Arc::new(SubflowHandler),
        Arc::new(ConfigHandler),
        Arc::new(EndHandler),
        Arc::new(FailHandler),
    ]
}
