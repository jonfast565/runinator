use std::collections::HashMap;

use runinator_models::value::{Map, Value};

mod compute;
mod conditions;
mod errors;
mod expressions;
mod keys;
mod normalize;
mod parameters;
mod refs;
mod run_state;
mod types;
mod typing;
mod validation;

pub use compute::{
    ComputeOutcome, ComputeProgram, ComputeStmt, EFFECTFUL_INTRINSIC_NAMES, HIGHER_ORDER_NAMES,
    IntrinsicLibrary, PureIntrinsics, call_pure, effectful_signatures, intrinsic_arity,
    intrinsic_signature, is_higher_order, is_known_intrinsic, parse_program, run_program,
};
pub use conditions::{evaluate_condition, evaluate_condition_with, next_transition};
pub use errors::{WorkflowTypeDiagnostic, WorkflowValidationError};
pub use expressions::{apply_input_defaults, resolve_value_refs, resolve_value_refs_pure};
pub use normalize::{normalize_definition, normalize_workflow};
pub use parameters::{
    evaluate_switch, parse_approval_parameters, parse_emit_parameters, parse_join_parameters,
    parse_loop_items, parse_map_parameters, parse_parallel_parameters, parse_race_parameters,
    parse_switch_parameters, parse_try_parameters, parse_wait_parameters,
};
pub use refs::expand_workflow_refs;
pub use run_state::{
    branch_policy_name, join_satisfied, latest_node_run, latest_status, race_winner,
};
pub use types::{
    ApprovalParameters, BranchPolicy, EmitParameters, JoinParameters, LoopParameters,
    MapParameters, ParallelParameters, RaceParameters, SwitchCase, SwitchParameters, TryParameters,
    WaitParameters, WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef,
};
pub use typing::{WorkflowType, validate_workflow_types};
pub use validation::{
    parse_nodes, validate_workflow, validate_workflow_with_config, validate_workflow_with_providers,
};

pub fn outputs_context(parameters: &Value, outputs: &HashMap<String, Value>) -> Value {
    let mut steps = Map::new();
    for (node, output) in outputs {
        steps.insert(node.clone(), runinator_models::json!({ "output": output }));
    }
    runinator_models::json!({
        "input": parameters,
        "steps": steps
    })
}

#[cfg(test)]
mod compute_tests;
#[cfg(test)]
mod tests;
