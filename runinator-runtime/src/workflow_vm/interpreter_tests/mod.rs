//! interpreter and durable-boundary regressions.

use super::effects::resolve_effect_request;
use super::interrupts::{interrupt_handler_continuation, resolve_interrupt};
use super::*;
use runinator_models::interrupt::{InterruptMode, InterruptSource};
use runinator_models::value::Value;
use runinator_models::workflow_state::DebugConfig;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowContinuationStatus, WorkflowEffectRequest, WorkflowFailure,
    WorkflowFailureKind, WorkflowFrame, WorkflowInstruction, WorkflowInterruptOutcome,
    WorkflowModule,
};
use runinator_models::{
    orchestration::GateKind,
    workflows::{
        WorkflowDefinition, WorkflowGraph, WorkflowNode, WorkflowNodeKind, WorkflowNodeRef,
        WorkflowTransitions,
    },
};
use runinator_workflows::compile_workflow_module;
use uuid::Uuid;

fn continuation() -> WorkflowContinuation {
    WorkflowContinuation::start(Uuid::now_v7(), 1)
}

fn interrupt_module() -> WorkflowModule {
    // [0] enter    [1] check    [2] effect    [3] return
    // [4] handler: const  [5] resume(mode)
    let mut module = WorkflowModule::new(vec![
        WorkflowInstruction::EnterNode {
            node_id: "call".into(),
        },
        WorkflowInstruction::CheckInterrupt {
            handlers: vec![runinator_models::workflow_vm::WorkflowVmInterruptHandler {
                source: InterruptSource::External,
                target: 4,
                timer_id: None,
                interval_seconds: None,
            }],
        },
        WorkflowInstruction::Effect {
            request: WorkflowEffectRequest::TimerDelay { seconds: 1 },
        },
        WorkflowInstruction::Return,
        WorkflowInstruction::Const {
            value: Value::from("handled"),
        },
        WorkflowInstruction::ResumeInterrupt {
            mode: InterruptMode::Resume,
        },
    ]);
    module.source_map = vec![runinator_models::workflow_vm::WorkflowSourceMapEntry {
        version: runinator_models::workflow_vm::WORKFLOW_SOURCE_MAP_VERSION,
        instruction_start: 0,
        instruction_end: 4,
        node_id: "call".into(),
        edge_label: None,
        interruptible: true,
        exit_instruction_pointer: Some(3),
    }];
    module
}

mod effects;
mod execution;
mod interrupts;
mod parallel;
mod structured;
