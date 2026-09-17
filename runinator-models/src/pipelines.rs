use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::replicas::{TriggerActorType, TriggerSourceKind};
use crate::schedules::WorkflowConcurrency;
use crate::value::Value;
use crate::workflows::{WorkflowRun, WorkflowStatus, WorkflowTriggerKind};

/// what happens to downstream links when a member workflow fails. authoring-only: it seeds the
/// `on` selector of newly drawn links (`Halt` -> fire on success, `Continue` -> fire on complete).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineFailurePolicy {
    /// stop the pipeline when a step fails (new links default to firing on success).
    #[default]
    Halt,
    /// keep going when a step fails (new links default to firing on complete).
    Continue,
}

impl PipelineFailurePolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineFailurePolicy::Halt => "halt",
            PipelineFailurePolicy::Continue => "continue",
        }
    }
}

/// what happens to the *pipeline run* when one of its member workflows fails, evaluated per graph
/// member (falling back to [`PipelineDefaults::default_failure_mode`] during import). Named after PowerShell's `$ErrorActionPreference`,
/// whose `Stop`/`Continue`/`SilentlyContinue`/`Inquire` values this mirrors one-for-one. Unlike
/// [`PipelineFailurePolicy`] (which only seeds a newly-drawn link's `on` selector), this is enforced
/// at runtime by the chaining/settlement orchestration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMemberFailureMode {
    /// the failed member fires none of its outgoing links; the pipeline run still settles once
    /// every already-started member quiesces, and this failure counts toward that settlement.
    Stop,
    /// the failed member's outgoing links still fire per their own `on` selector (today's
    /// behavior), and this failure counts toward the pipeline run's settlement. the default, so an
    /// existing pipeline's behavior is unchanged by this setting's introduction.
    #[default]
    Continue,
    /// like `Continue`, but this member's failure alone does not fail the pipeline run's
    /// settlement (another member's `Stop`/`Continue` failure still can).
    SilentlyContinue,
    /// the failed member fires none of its outgoing links until a human resolves the pipeline
    /// run's pending inquiry (continue or abort); the pipeline run pauses (`approval_required`)
    /// rather than settling while the inquiry is open.
    Inquire,
}

impl PipelineMemberFailureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineMemberFailureMode::Stop => "stop",
            PipelineMemberFailureMode::Continue => "continue",
            PipelineMemberFailureMode::SilentlyContinue => "silently_continue",
            PipelineMemberFailureMode::Inquire => "inquire",
        }
    }
}

fn default_true() -> bool {
    true
}

/// which terminal state of a source member fires the link to the next member. mirrors the `on`
/// selector of a `chained` workflow trigger (`success` / `complete` / `failure`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineLinkSelector {
    /// fire only when the source run succeeds.
    #[default]
    Success,
    /// fire when the source run reaches any terminal state.
    Complete,
    /// fire only when the source run fails or times out.
    Failure,
}

impl PipelineLinkSelector {
    /// the chained-trigger `on` string this selector maps to.
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineLinkSelector::Success => "success",
            PipelineLinkSelector::Complete => "complete",
            PipelineLinkSelector::Failure => "failure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineJoinMode {
    #[default]
    All,
    Any,
    FirstSuccess,
}

/// a bare name is a member with no failure-mode override (takes the pipeline default at import).

// pipelinedefaults derives clone but not partialeq; pipelinespec's partialeq needs it.

pub const PIPELINE_GRAPH_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineMemberAttemptStatus {
    Pending,
    Queued,
    Running,
    Waiting,
    ApprovalRequired,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
    Skipped,
}

impl PipelineMemberAttemptStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::ApprovalRequired => "approval_required",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Canceled => "canceled",
            Self::Skipped => "skipped",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::TimedOut | Self::Canceled | Self::Skipped
        )
    }
}

impl TryFrom<&str> for PipelineMemberAttemptStatus {
    type Error = String;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "waiting" => Ok(Self::Waiting),
            "approval_required" => Ok(Self::ApprovalRequired),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "timed_out" => Ok(Self::TimedOut),
            "canceled" => Ok(Self::Canceled),
            "skipped" => Ok(Self::Skipped),
            other => Err(format!("unknown pipeline member attempt status {other}")),
        }
    }
}

mod pipeline_defaults;
pub use pipeline_defaults::PipelineDefaults;

mod pipeline_link_spec;
pub use pipeline_link_spec::PipelineLinkSpec;

mod pipeline_join_spec;
pub use pipeline_join_spec::PipelineJoinSpec;

mod pipeline_spec;
pub use pipeline_spec::PipelineSpec;

mod pipeline_member_spec;
pub use pipeline_member_spec::PipelineMemberSpec;

mod pipeline_trigger_spec;
pub use pipeline_trigger_spec::PipelineTriggerSpec;

mod pipeline_bundle;
pub use pipeline_bundle::PipelineBundle;

mod pipeline_member;
pub use pipeline_member::PipelineMember;

mod pipeline_link;
pub use pipeline_link::PipelineLink;

mod pipeline_join;
pub use pipeline_join::PipelineJoin;

mod pipeline_graph;
pub use pipeline_graph::PipelineGraph;

mod pipeline;
pub use pipeline::Pipeline;

mod pipeline_member_attempt;
pub use pipeline_member_attempt::PipelineMemberAttempt;

mod pipeline_run_edge_state;
pub use pipeline_run_edge_state::PipelineRunEdgeState;

mod pipeline_run_join_state;
pub use pipeline_run_join_state::PipelineRunJoinState;

mod pipeline_trigger;
pub use pipeline_trigger::PipelineTrigger;

mod pipeline_run;
pub use pipeline_run::PipelineRun;

mod pipeline_execution_context;
pub use pipeline_execution_context::PipelineExecutionContext;

mod pipeline_run_detail;
pub use pipeline_run_detail::PipelineRunDetail;
