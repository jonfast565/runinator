use std::convert::TryFrom;

use prost::Enumeration;
use prost::Message;

use crate::ControlKind;

#[derive(Clone, PartialEq, Message)]
pub struct WorkerControlEvent {
    #[prost(string, tag = "1")]
    pub worker_id: String,
    #[prost(enumeration = "WorkerControlEventKind", tag = "2")]
    pub kind: i32,
    #[prost(int64, optional, tag = "3")]
    pub workflow_run_id: Option<i64>,
    #[prost(int64, optional, tag = "4")]
    pub workflow_node_run_id: Option<i64>,
    #[prost(string, tag = "5")]
    pub node_id: String,
    #[prost(enumeration = "WorkerControlActionKind", optional, tag = "6")]
    pub control_kind: Option<i32>,
    #[prost(string, tag = "7")]
    pub message: String,
    #[prost(int64, tag = "8")]
    pub timestamp_millis: i64,
}

#[derive(Clone, PartialEq, Message)]
pub struct SchedulerControlAck {
    #[prost(bool, tag = "1")]
    pub accepted: bool,
    #[prost(string, tag = "2")]
    pub message: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum WorkerControlEventKind {
    WorkerStarted = 0,
    WorkerStopping = 1,
    ActionStarted = 2,
    ActionFinished = 3,
    ControlRequested = 4,
    ControlApplied = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Enumeration)]
#[repr(i32)]
pub enum WorkerControlActionKind {
    Cancel = 0,
    Pause = 1,
    Resume = 2,
}

impl WorkerControlEvent {
    pub fn new(
        worker_id: impl Into<String>,
        kind: WorkerControlEventKind,
        timestamp_millis: i64,
    ) -> Self {
        Self {
            worker_id: worker_id.into(),
            kind: kind as i32,
            workflow_run_id: None,
            workflow_node_run_id: None,
            node_id: String::new(),
            control_kind: None,
            message: String::new(),
            timestamp_millis,
        }
    }

    pub fn with_workflow_run_id(mut self, workflow_run_id: i64) -> Self {
        self.workflow_run_id = Some(workflow_run_id);
        self
    }

    pub fn with_workflow_node_run_id(mut self, workflow_node_run_id: i64) -> Self {
        self.workflow_node_run_id = Some(workflow_node_run_id);
        self
    }

    pub fn with_node_id(mut self, node_id: impl Into<String>) -> Self {
        self.node_id = node_id.into();
        self
    }

    pub fn with_control_kind(mut self, control_kind: ControlKind) -> Self {
        self.control_kind = Some(WorkerControlActionKind::from(control_kind) as i32);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = message.into();
        self
    }
}

impl SchedulerControlAck {
    pub fn accepted(message: impl Into<String>) -> Self {
        Self {
            accepted: true,
            message: message.into(),
        }
    }

    pub fn rejected(message: impl Into<String>) -> Self {
        Self {
            accepted: false,
            message: message.into(),
        }
    }
}

impl From<ControlKind> for WorkerControlActionKind {
    fn from(value: ControlKind) -> Self {
        match value {
            ControlKind::Cancel => Self::Cancel,
            ControlKind::Pause => Self::Pause,
            ControlKind::Resume => Self::Resume,
        }
    }
}

impl TryFrom<WorkerControlActionKind> for ControlKind {
    type Error = &'static str;

    fn try_from(value: WorkerControlActionKind) -> Result<Self, Self::Error> {
        match value {
            WorkerControlActionKind::Cancel => Ok(Self::Cancel),
            WorkerControlActionKind::Pause => Ok(Self::Pause),
            WorkerControlActionKind::Resume => Ok(Self::Resume),
        }
    }
}
