use std::collections::HashMap;

use runinator_models::value::{Map, Value};

mod catalog;
mod compute;
mod conditions;
mod errors;
mod expressions;
mod functions;
mod keys;
mod normalize;
mod parameters;
mod refs;
mod run_state;
mod types;
mod typing;
mod validation;

pub use catalog::{enum_catalogs, node_kind_catalog, trigger_kind_catalog};
pub use compute::{
    ComputeOutcome, ComputeProgram, ComputeStmt, EFFECTFUL_INTRINSIC_NAMES, HIGHER_ORDER_NAMES,
    IntrinsicLibrary, PureIntrinsics, STD_MODULES, STD_NAMESPACE, call_pure, effectful_signatures,
    intrinsic_arity, intrinsic_module, intrinsic_signature, is_higher_order, is_known_intrinsic,
    parse_program, qualified_intrinsic_name, resolve_std_path, run_program, run_program_with,
};
pub use conditions::{
    evaluate_condition, evaluate_condition_with, next_transition, validate_condition_value,
};
pub use errors::{WorkflowTypeDiagnostic, WorkflowValidationError};
pub use expressions::{
    apply_input_defaults, resolve_value_refs, resolve_value_refs_pure,
    resolve_value_refs_with_functions, validate_expression,
};
pub use functions::{FunctionTable, RuntimeFunction, intrinsic_catalog};
pub use normalize::{normalize_definition, normalize_workflow};
pub use parameters::{
    evaluate_percentage, evaluate_switch, evaluate_toggle, parse_approval_parameters,
    parse_gate_parameters, parse_input_parameters, parse_join_parameters, parse_loop_items,
    parse_map_parameters, parse_output_parameters, parse_parallel_parameters,
    parse_percentage_parameters, parse_race_parameters, parse_signal_parameters,
    parse_switch_parameters, parse_toggle_parameters, parse_try_parameters, parse_wait_parameters,
};
pub use refs::expand_workflow_refs;
pub use run_state::{
    branch_policy_name, join_satisfied, latest_node_run, latest_status, race_winner,
};
pub use types::{
    ApprovalParameters, ArtifactItem, BranchPolicy, GateParameters, InputParameters,
    JoinParameters, LoopParameters, MapParameters, OutputParameters, ParallelParameters,
    PercentageBucket, PercentageParameters, RaceParameters, SignalParameters, SwitchCase,
    SwitchParameters, ToggleParameters, TryParameters, WaitParameters, WorkflowExpression,
    WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef,
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
mod functions_tests;
#[cfg(test)]
mod tests;
