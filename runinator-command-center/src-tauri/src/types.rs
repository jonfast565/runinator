use chrono::{DateTime, Utc};

use runinator_models::settings::SettingKind;
use runinator_models::value::Value;
use runinator_models::workflow_vm::{
    WorkflowContinuation, WorkflowEffect, WorkflowJournalRecord, WorkflowVmCursor,
};
use runinator_models::workflows::{WorkflowNodeRun, WorkflowRun};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

fn default_service_scheme() -> String {
    "http".to_string()
}

mod service_status;
pub use service_status::ServiceStatus;

mod web_service_announcement;
pub use web_service_announcement::WebServiceAnnouncement;

mod workflow_run_detail;
pub use workflow_run_detail::WorkflowRunDetail;

mod workflow_run_created;
pub use workflow_run_created::WorkflowRunCreated;

mod credential_summary;
pub use credential_summary::CredentialSummary;

mod credential_put_request;
pub use credential_put_request::CredentialPutRequest;

mod diagnostic_summary;
pub use diagnostic_summary::DiagnosticSummary;
