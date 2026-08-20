use std::collections::HashMap;

use chrono::Utc;
use runinator_comm::{ActionCommand, WireCodec};
use runinator_models::{
    cursor::RunCursor,
    errors::SendableError,
    orchestration::{GateKind, NewOrchestrationEvent},
    value::{Map, Value},
    workflow_state::{
        ApprovalRecord, ApprovalState, AssertOutput, AuditOutput, AwaitWorkflowOutput,
        AwaitWorkflowState, BarrierOutput, BarrierState, CheckpointOutput, CircuitBreakerOutput,
        CollectOutput, CollectState, CompensationFrame, ConfigSummary, CooldownOutput,
        DebounceOutput, DebounceState, EventSourceState, GateRecord, GateState, InputState,
        JoinOutput, LoopFrame, LoopOutput, MapChild, MapChildState, MapFrame, MapOutput,
        MutexOutput, MutexState, OutputPayload, ParallelOutput, RaceOutput, SignalState,
        SkippedOutput, SubflowOutcome, SubflowState, SwitchOutput, ThrottleOutput, ThrottleState,
        TransformOutput, TryFrame, WaitElapsedOutput, WaitState, WorkflowContextHeader,
        WorkflowRunState,
    },
    workflows::{
        WorkflowAction, WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowRun, WorkflowStatus, WorkflowSubflowType, WorkflowTaskRun,
    },
};
use runinator_store::RuntimeStore;
use runinator_workflows::{branch_policy_name, join_satisfied, latest_status, race_winner_since};
use uuid::Uuid;

/// Ready-queue ownership decision retained for engine compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadyNodeDisposition {
    Complete,
    KeepClaim,
}

mod action;
mod approval;
mod assert;
mod audit;
mod await_run;
mod barrier;
mod basic;
mod chaining;
mod checkpoint;
mod circuit_breaker;
mod collect;
mod compensation;
mod context;
mod control_flow;
mod cooldown;
mod debounce;
mod debug;
mod event_source;
mod execution;
mod gate;
mod input;
pub(crate) mod interpreter;
mod interrupt;
mod invocation;
mod map;
mod mutex;
mod output;
mod pipeline_orchestration;
mod run_state;
mod signal;
mod subflow;
mod throttle;
mod transform;
mod transitions;
mod wait;

#[cfg(test)]
mod action_tests;
#[cfg(test)]
mod chaining_tests;
#[cfg(test)]
mod control_flow_tests;
#[cfg(test)]
mod cooldown_tests;
#[cfg(test)]
mod debug_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod invocation_tests;
#[cfg(test)]
mod mutex_tests;
#[cfg(test)]
mod runtime_tests;

#[cfg(test)]
mod interrupt_tests;
#[cfg(test)]
mod orchestration_tests;
#[cfg(test)]
mod stacked_control_flow_tests;

pub use pipeline_orchestration::{
    PipelineInquiryDecision, PipelineStartOutcome, create_and_start_pipeline_run,
    resolve_pipeline_run_inquiry, resume_pipeline_run, retry_pipeline_member, start_pipeline_run,
};
