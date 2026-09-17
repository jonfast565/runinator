use runinator_models::invocation::InvocationModule;
use runinator_models::orchestration::GateKind;
use runinator_models::value::Value;
use runinator_models::workflow_ast::WorkflowExpression;
use runinator_models::workflows::{WorkflowCondition, WorkflowNodeRef};

// the expression/ref/compute program ast now lives in `runinator_models::workflow_ast` so
// `WorkflowNode` fields can be typed against it; this module keeps the per-node parameter structs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BranchPolicy {
    All,
    Any,
    FirstSuccess,
}

impl BranchPolicy {
    pub fn parse(value: Option<&Value>, default: BranchPolicy) -> Result<Self, String> {
        match value.and_then(Value::as_str) {
            None => Ok(default),
            Some("all") => Ok(BranchPolicy::All),
            Some("any") => Ok(BranchPolicy::Any),
            Some("first_success") => Ok(BranchPolicy::FirstSuccess),
            Some(other) => Err(format!("unsupported branch policy '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateTimeoutPolicy {
    Fail,
    Continue,
}

mod switch_case;
pub use switch_case::SwitchCase;

mod switch_parameters;
pub use switch_parameters::SwitchParameters;

mod toggle_parameters;
pub use toggle_parameters::ToggleParameters;

mod percentage_parameters;
pub use percentage_parameters::PercentageParameters;

mod percentage_bucket;
pub use percentage_bucket::PercentageBucket;

mod parallel_parameters;
pub use parallel_parameters::ParallelParameters;

mod join_parameters;
pub use join_parameters::JoinParameters;

mod try_parameters;
pub use try_parameters::TryParameters;

mod map_parameters;
pub use map_parameters::MapParameters;

mod race_parameters;
pub use race_parameters::RaceParameters;

mod output_parameters;
pub use output_parameters::OutputParameters;

mod invocation_parameters;
pub use invocation_parameters::InvocationParameters;

mod artifact_item;
pub use artifact_item::ArtifactItem;

mod input_parameters;
pub use input_parameters::InputParameters;

mod wait_parameters;
pub use wait_parameters::WaitParameters;

mod approval_parameters;
pub use approval_parameters::ApprovalParameters;

mod signal_parameters;
pub use signal_parameters::SignalParameters;

mod gate_parameters;
pub use gate_parameters::GateParameters;

mod loop_parameters;
pub use loop_parameters::LoopParameters;
