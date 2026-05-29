use std::collections::HashMap;

use serde_json::{Map, Value};

mod conditions;
mod errors;
mod expressions;
mod normalize;
mod parameters;
mod refs;
mod types;
mod typing;
mod validation;

pub use conditions::{evaluate_condition, next_transition};
pub use errors::{WorkflowTypeDiagnostic, WorkflowValidationError};
pub use expressions::resolve_value_refs;
pub use normalize::{normalize_definition, normalize_workflow};
pub use parameters::{
    evaluate_switch, parse_approval_parameters, parse_emit_parameters, parse_join_parameters,
    parse_loop_items, parse_map_parameters, parse_parallel_parameters, parse_race_parameters,
    parse_switch_parameters, parse_try_parameters, parse_wait_parameters,
};
pub use refs::expand_workflow_refs;
pub use types::{
    ApprovalParameters, BranchPolicy, EmitParameters, JoinParameters, LoopParameters,
    MapParameters, ParallelParameters, RaceParameters, SwitchCase, SwitchParameters, TryParameters,
    WaitParameters, WorkflowExpression, WorkflowPathSegment, WorkflowRefSource, WorkflowValueRef,
};
pub use typing::{WorkflowType, validate_workflow_types};
pub use validation::{parse_nodes, validate_workflow, validate_workflow_with_providers};

pub fn outputs_context(parameters: &Value, outputs: &HashMap<String, Value>) -> Value {
    let mut steps = Map::new();
    for (node, output) in outputs {
        steps.insert(node.clone(), serde_json::json!({ "output": output }));
    }
    serde_json::json!({
        "input": parameters,
        "steps": steps
    })
}

#[cfg(test)]
mod tests;
