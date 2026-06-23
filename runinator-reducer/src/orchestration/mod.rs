#![allow(unused_imports)]
use std::collections::HashMap;

use chrono::Utc;
use runinator_comm::{ActionCommand, WireCodec};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    errors::SendableError,
    orchestration::{GateKind, NewOrchestrationEvent, ReadyNodeRecord},
    value::{Map, Value},
    workflow_state::{
        ApprovalRecord, ApprovalState, AssertOutput, AssertViolation, AuditOutput, AwaitRunOutput,
        AwaitRunState, BarrierOutput, BarrierState, CheckpointOutput, CircuitBreakerOutput,
        CircuitBreakerState, CollectOutput, CollectState, CompensationFrame, ConfigSummary,
        DebounceOutput, DebounceState, EventSourceState, GateRecord, GateState, InputState,
        JoinOutput, LoopFrame, LoopOutput, MapChild, MapChildState, MapFrame, MapOutput,
        MutexOutput, MutexState, OutputPayload, ParallelFrame, ParallelOutput, RaceFrame,
        RaceOutput, SignalState, SkippedOutput, SubflowOutcome, SubflowState, SwitchOutput,
        ThrottleOutput, ThrottleState, TransformOutput, TryFrame, WaitElapsedOutput, WaitState,
        WorkflowContextHeader, WorkflowRunState,
    },
    workflows::{
        WorkflowAction, WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowNodeRunArtifact,
        WorkflowRun, WorkflowStatus, WorkflowSubflowType,
    },
};
use runinator_workflows::{branch_policy_name, join_satisfied, latest_status, race_winner};
use uuid::Uuid;

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
mod checkpoint;
mod circuit_breaker;
mod collect;
mod compensation;
mod compute;
mod context;
mod control_flow;
mod debounce;
mod engine;
mod event_source;
mod gate;
mod input;
mod map;
mod mutex;
mod output;
mod signal;
mod subflow;
mod throttle;
mod transform;
mod transitions;
mod wait;

#[cfg(test)]
mod tests;

pub use engine::process_ready_node;
