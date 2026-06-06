#![allow(unused_imports)]
use std::collections::HashMap;

use chrono::Utc;
use runinator_comm::{ActionCommand, WireCodec};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    errors::SendableError,
    orchestration::{NewOrchestrationEvent, ReadyNodeRecord},
    value::Value,
    workflow_state::{
        ApprovalRecord, ApprovalState, ConfigSummary, EmitOutput, JoinOutput, LoopFrame,
        LoopOutput, MapFrame, MapOutput, ParallelFrame, ParallelOutput, RaceFrame, RaceOutput,
        SkippedOutput, SubflowOutcome, SubflowState, SwitchOutput, TryFrame, WaitElapsedOutput,
        WaitState, WorkflowContextHeader, WorkflowRunState,
    },
    workflows::{
        WorkflowAction, WorkflowNode, WorkflowNodeKind, WorkflowNodeRun, WorkflowRun,
        WorkflowStatus, WorkflowSubflowType,
    },
};
use runinator_workflows::{
    append_completed_map_item, branch_policy_name, join_satisfied, latest_status, race_winner,
};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadyNodeDisposition {
    Complete,
    KeepClaim,
}

mod action;
mod approval;
mod basic;
mod context;
mod control_flow;
mod engine;
mod subflow;
mod transitions;
mod wait;

pub(crate) use engine::process_ready_node;
