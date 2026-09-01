//! persistence facade for scheduling views that span definitions, identity, and subscriptions.

use runinator_models::{
    auth::{PrincipalKind, User},
    errors::SendableError,
    pipelines::{Pipeline, PipelineTrigger},
    rbac::RoleAssignment,
    schedules::{CalendarSubscription, NewCalendarSubscriptionRecord},
    workflows::WorkflowDefinition,
};
use runinator_store::roles::{AuthStore, DefinitionStore, RbacStore, ScheduleStore};
use uuid::Uuid;

pub async fn create_calendar_subscription<T: ScheduleStore>(
    db: &T,
    record: &NewCalendarSubscriptionRecord,
) -> Result<CalendarSubscription, SendableError> {
    db.create_calendar_subscription(record).await
}

pub async fn fetch_calendar_subscription_by_hash<T: ScheduleStore>(
    db: &T,
    token_hash: String,
) -> Result<Option<CalendarSubscription>, SendableError> {
    db.fetch_calendar_subscription_by_hash(token_hash).await
}

pub async fn delete_calendar_subscription<T: ScheduleStore>(
    db: &T,
    subscription_id: Uuid,
    principal_id: Uuid,
) -> Result<bool, SendableError> {
    db.delete_calendar_subscription(subscription_id, principal_id)
        .await
}

pub async fn fetch_calendar_user<T: AuthStore>(
    db: &T,
    user_id: Uuid,
) -> Result<Option<User>, SendableError> {
    db.fetch_user(user_id).await
}

pub async fn fetch_calendar_role_assignments<T: RbacStore>(
    db: &T,
    user_id: Uuid,
) -> Result<Vec<RoleAssignment>, SendableError> {
    db.list_principal_role_assignments(PrincipalKind::User, user_id)
        .await
}

pub async fn fetch_calendar_workflows<T: DefinitionStore>(
    db: &T,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    super::fetch_workflows(db).await
}

pub async fn fetch_calendar_pipelines<T: DefinitionStore>(
    db: &T,
) -> Result<Vec<Pipeline>, SendableError> {
    super::fetch_pipelines(db).await
}

pub async fn fetch_calendar_pipeline_triggers<T: ScheduleStore>(
    db: &T,
    pipeline_id: Uuid,
) -> Result<Vec<PipelineTrigger>, SendableError> {
    super::fetch_pipeline_triggers(db, pipeline_id).await
}
