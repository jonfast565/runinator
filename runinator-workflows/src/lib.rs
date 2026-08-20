use std::collections::HashMap;

use runinator_models::value::{Map, Value};

mod catalog;
mod node_kinds;
mod normalize;
mod parameters;
mod refs;
mod run_state;
mod simulate;
mod testkit;
mod types;
mod typing;
mod validation;

pub use catalog::{enum_catalogs, node_kind_catalog, node_metadata, trigger_kind_catalog};
// the expression/compute language lives in `runinator-compute`; re-exported here at its historical
// `runinator_workflows::…` paths so graph-layer consumers need not name both crates. a consumer
// that only evaluates values (a provider, the rexrap front end) should depend on `runinator-compute`
// directly rather than pulling the graph layer in for these.
pub use node_kinds::{
    ActionCatalog, GraphRole, NodeKindSpec, TargetRule, TargetSlot, graph_role, spec_for,
    target_slots,
};
pub use normalize::{normalize_definition, normalize_workflow};
pub use parameters::{
    evaluate_percentage, evaluate_switch, evaluate_toggle, parse_approval_parameters,
    parse_gate_parameters, parse_input_parameters, parse_invocation_parameters,
    parse_join_parameters, parse_loop_parameters, parse_map_parameters, parse_output_parameters,
    parse_parallel_parameters, parse_percentage_parameters, parse_race_parameters,
    parse_signal_parameters, parse_switch_parameters, parse_toggle_parameters,
    parse_try_parameters, parse_wait_parameters,
};
pub use refs::expand_workflow_refs;
pub use run_state::{
    branch_policy_name, join_satisfied, latest_node_run, latest_status, race_winner,
    race_winner_since,
};
pub use runinator_compute::{
    CallableCatalog, EFFECTFUL_INTRINSIC_NAMES, FunctionTable, HIGHER_ORDER_NAMES,
    IntrinsicLibrary, PureIntrinsics, RuntimeFunction, STD_MODULES, STD_NAMESPACE, VmEnv,
    WorkflowTypeDiagnostic, WorkflowValidationError, apply_input_defaults, assemble_program,
    call_pure, effectful_signatures, evaluate_condition, evaluate_condition_with,
    evaluate_expression, evaluate_workflow_condition, intrinsic_arity, intrinsic_catalog,
    intrinsic_module, intrinsic_result_type, intrinsic_signature, is_higher_order,
    is_known_intrinsic, next_transition, parse_program, qualified_intrinsic_name, resolve_std_path,
    resolve_value_refs, resolve_value_refs_pure, resolve_value_refs_with_functions, start,
    validate_condition_value, validate_expression,
};
pub use runinator_models::workflow_ast::{
    ComputeProgram, ComputeStmt, WorkflowExpression, WorkflowPathSegment, WorkflowRefSource,
    WorkflowValueRef,
};
pub use simulate::{
    NodeEvalRequest, NodeOutcome, SimStep, SimulationEnv, SimulationRun, simulate_workflow,
};
pub use testkit::{
    Expectations, MockEnv, MockSpec, TestCaseResult, WorkflowTestCase, WorkflowTestSuite,
    run_test_case,
};
pub use types::{
    ApprovalParameters, ArtifactItem, BranchPolicy, GateParameters, GateTimeoutPolicy,
    InputParameters, JoinParameters, LoopParameters, MapParameters, OutputParameters,
    ParallelParameters, PercentageBucket, PercentageParameters, RaceParameters, SignalParameters,
    SwitchCase, SwitchParameters, ToggleParameters, TryParameters, WaitParameters,
};
pub use typing::{WorkflowType, validate_workflow_types};
pub use validation::{
    interrupt_declarations, interrupt_declarations_for, interrupt_region,
    interrupt_region_is_supported, interrupt_region_nodes, parse_nodes, validate_workflow,
    validate_workflow_with_config, validate_workflow_with_providers,
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
mod conformance_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod workflow_ast_tests;
