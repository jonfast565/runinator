//! output behavior for the workflow interpreter.

use super::InstructionOutcome;
use super::context::local_context;
use super::failure::handle_failure;
use runinator_models::value::Value;
use runinator_models::workflow_vm::{WorkflowContinuation, WorkflowModule};
use std::ops::ControlFlow::{Break, Continue};

pub(super) fn set_output(
    mut continuation: WorkflowContinuation,
    module: &WorkflowModule,
    event_type: &Option<String>,
    artifacts: &[runinator_models::workflow_vm::WorkflowOutputArtifact],
) -> InstructionOutcome {
    let context = local_context(&continuation);
    let mut artifact_values = runinator_models::value::Map::new();
    for artifact in artifacts {
        let value = match runinator_compute::evaluate_module_pure(
            &artifact.source,
            &context,
            &runinator_compute::CallableCatalog::builtin(),
        ) {
            Ok(value) => value,
            Err(error) => {
                return Break(handle_failure(module, continuation, error.to_string()));
            }
        };
        artifact_values.insert(artifact.name.clone(), value);
    }
    let data = continuation.stack.last().cloned().unwrap_or(Value::Null);
    let mut output = runinator_models::value::Map::new();
    output.insert("data".into(), data);
    output.insert("artifacts".into(), Value::Object(artifact_values));
    if let Some(event_type) = event_type {
        output.insert("event_type".into(), Value::String(event_type.clone()));
    }
    continuation
        .locals
        .insert("__workflow_vm_output".into(), Value::Object(output));
    continuation.instruction_pointer += 1;
    Continue(continuation)
}
