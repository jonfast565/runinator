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
        ApprovalRecord, ApprovalState, CompensationFrame, ConfigSummary, GateRecord, GateState,
        InputState, JoinOutput, LoopFrame, LoopOutput, MapChild, MapChildState, MapFrame,
        MapOutput, OutputPayload, ParallelFrame, ParallelOutput, RaceFrame, RaceOutput,
        SignalState, SkippedOutput, SubflowOutcome, SubflowState, SwitchOutput, TryFrame,
        WaitElapsedOutput, WaitState, WorkflowContextHeader, WorkflowRunState,
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
mod basic;
mod compensation;
mod compute;
mod context;
mod control_flow;
mod deliverable;
mod engine;
mod gate;
mod input;
mod map;
mod output;
mod signal;
mod subflow;
mod transitions;
mod wait;

pub use engine::process_ready_node;
