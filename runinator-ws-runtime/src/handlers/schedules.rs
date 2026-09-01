use std::sync::Arc;
use uuid::Uuid;

use axum::{
    Extension, Json,
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use runinator_models::{
    auth::PrincipalKind,
    auth::ResourceType,
    auth::{AuthContext, Permission},
    rbac::{Action, Role, ScopeKind, ScopeRef},
    schedules::{
        BackfillRequest, CalendarSubscriptionSecret, NewCalendarSubscriptionRecord, NewFreezeWindow,
    },
    schedules::{ScheduleRecurrence, ScheduleSpec},
    validation::{Validate, ValidationError},
    workflows::WorkflowTriggerKind,
};
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ScheduleStore},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use runinator_engine::services::SchedulingOperations;
use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker};

type Reply = (StatusCode, Json<ApiResponse>);

#[derive(Debug, Clone, Copy, Deserialize, Default)]
pub struct FreezeWindowsQuery {
    /// narrow to one org's windows; the platform-wide ones are always included, since those are
    /// what actually freeze that org's schedules.
    #[serde(default)]
    pub org_id: Option<Uuid>,
    /// list only the windows in effect right now.
    #[serde(default)]
    pub active: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CalendarScope {
    Platform,
    Organization,
    #[default]
    User,
}

#[derive(Deserialize, Default)]
pub struct CalendarQuery {
    #[serde(default)]
    pub scope: CalendarScope,
    #[serde(default)]
    pub org_id: Option<Uuid>,
    #[serde(default)]
    pub horizon_days: Option<i64>,
}

#[derive(Deserialize)]
pub struct CalendarSubscriptionRequest {
    pub scope: CalendarScope,
    #[serde(default)]
    pub org_id: Option<Uuid>,
}

impl Validate for CalendarSubscriptionRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

pub async fn create_calendar_subscription<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CalendarSubscriptionRequest>,
) -> Response {
    let Some(principal_id) = ctx.principal_id.filter(|_| ctx.kind == PrincipalKind::User) else {
        return api_error("calendar subscriptions require a user principal").into_response();
    };
    let scope = match request.scope {
        CalendarScope::Platform => ScopeRef::PLATFORM,
        CalendarScope::Organization => match request
            .org_id
            .or(ctx.org_id)
            .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        {
            Some(scope) => scope,
            None => return api_error("organization scope needs org_id").into_response(),
        },
        CalendarScope::User => ScopeRef::new(ScopeKind::User, Some(principal_id)).unwrap(),
    };
    if let Err(reply) = ctx.require_scope_action(Action::View, scope) {
        return reply.into_response();
    }
    let token = format!(
        "runi_cal_{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    );
    let record = NewCalendarSubscriptionRecord {
        id: Uuid::now_v7(),
        principal_id,
        scope,
        token_hash: sha256(&token),
        created_at: chrono::Utc::now(),
    };
    match service.create_calendar_subscription(&record).await {
        Ok(subscription) => Json(CalendarSubscriptionSecret {
            subscription,
            token,
        })
        .into_response(),
        Err(error) => api_error(error.to_string()).into_response(),
    }
}

pub async fn delete_calendar_subscription<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(subscription_id): Path<Uuid>,
) -> Response {
    let Some(principal_id) = ctx.principal_id else {
        return api_error("calendar subscriptions require a user principal").into_response();
    };
    match service
        .delete_calendar_subscription(subscription_id, principal_id)
        .await
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => {
            not_found(format!("Calendar subscription {subscription_id} not found")).into_response()
        }
        Err(error) => api_error(error.to_string()).into_response(),
    }
}

pub async fn subscribed_calendar<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Path(token): Path<String>,
) -> Response {
    let subscription = match service
        .fetch_calendar_subscription_by_hash(sha256(&token))
        .await
    {
        Ok(Some(value)) => value,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let user = match service.calendar_user(subscription.principal_id).await {
        Ok(Some(user)) if !user.disabled => user,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let assignments = match service
        .calendar_role_assignments(subscription.principal_id)
        .await
    {
        Ok(values) => values,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let platform_role = assignments
        .iter()
        .find_map(|assignment| match assignment.role {
            Role::Platform(role) => Some(role),
            _ => None,
        });
    let ctx = AuthContext {
        principal_id: user.id,
        session_id: None,
        kind: PrincipalKind::User,
        platform_role,
        assignments,
        system_role: None,
        action_ceiling: Vec::new(),
        org_id: (subscription.scope.kind == ScopeKind::Organization)
            .then_some(subscription.scope.id)
            .flatten(),
    };
    let scope = match subscription.scope.kind {
        ScopeKind::Platform => CalendarScope::Platform,
        ScopeKind::Organization => CalendarScope::Organization,
        ScopeKind::User | ScopeKind::Team => CalendarScope::User,
    };
    schedule_calendar::<T>(
        Extension(db),
        Extension(service),
        Extension(ctx),
        Query(CalendarQuery {
            scope,
            org_id: (subscription.scope.kind == ScopeKind::Organization)
                .then_some(subscription.scope.id)
                .flatten(),
            horizon_days: Some(180),
        }),
    )
    .await
}

fn sha256(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Outlook-compatible iCalendar export of every scheduled workflow/pipeline the caller can view.
pub async fn schedule_calendar<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<CalendarQuery>,
) -> Response {
    let requested_org = match query.scope {
        CalendarScope::Organization => query.org_id.or(ctx.org_id),
        CalendarScope::Platform | CalendarScope::User => None,
    };
    let scope = match query.scope {
        CalendarScope::Platform => ScopeRef::PLATFORM,
        CalendarScope::Organization => {
            match requested_org.and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id))) {
                Some(scope) => scope,
                None => return api_error("organization scope needs org_id").into_response(),
            }
        }
        CalendarScope::User => ctx
            .principal_id
            .and_then(|id| ScopeRef::new(ScopeKind::User, Some(id)))
            .unwrap_or_else(|| ctx.selected_scope()),
    };
    if !matches!(query.scope, CalendarScope::User)
        && let Err(reply) = ctx.require_scope_action(Action::View, scope)
    {
        return reply.into_response();
    }

    let checker = AuthzChecker::new(db.as_ref(), &ctx);
    let visible_workflows = match checker.visible_workflow_ids().await {
        Ok(ids) => ids,
        Err(reply) => return reply.into_response(),
    };
    let visible_pipelines = match checker.visible_resource_ids(ResourceType::Pipeline).await {
        Ok(ids) => ids,
        Err(reply) => return reply.into_response(),
    };
    let workflows = match service.workflows().await {
        Ok(values) => values,
        Err(error) => return api_error(error.to_string()).into_response(),
    };
    let pipelines = match service.pipelines().await {
        Ok(values) => values,
        Err(error) => return api_error(error.to_string()).into_response(),
    };
    let mut entries = Vec::new();
    for workflow in workflows {
        let Some(workflow_id) = workflow.id else {
            continue;
        };
        if visible_workflows
            .as_ref()
            .is_some_and(|ids| !ids.contains(&workflow_id))
            || requested_org.is_some_and(|id| workflow.org_id != Some(id))
        {
            continue;
        }
        let triggers = match service.list_workflow_triggers(workflow_id).await {
            Ok(values) => values,
            Err(error) => return api_error(error.to_string()).into_response(),
        };
        for trigger in triggers
            .into_iter()
            .filter(|trigger| trigger.enabled && trigger.kind == WorkflowTriggerKind::Cron)
        {
            let Some(trigger_id) = trigger.id else {
                continue;
            };
            let (schedule, exclusions) = match calendar_schedules(
                &trigger.configuration,
                trigger.blackout_start.zip(trigger.blackout_end),
            ) {
                Ok(value) => value,
                Err(error) => return api_error(error).into_response(),
            };
            entries.push(runinator_scheduling::ical::CalendarEntry {
                uid: format!("workflow-trigger-{trigger_id}"),
                summary: format!("Runinator · {}", workflow.name),
                description: format!(
                    "Scheduled workflow {}",
                    qualified_name(workflow.namespace.as_deref(), &workflow.name)
                ),
                schedule,
                exclusions,
            });
        }
    }
    for pipeline in pipelines {
        let Some(pipeline_id) = pipeline.id else {
            continue;
        };
        if visible_pipelines
            .as_ref()
            .is_some_and(|ids| !ids.contains(&pipeline_id))
            || requested_org.is_some_and(|id| pipeline.org_id != Some(id))
        {
            continue;
        }
        let triggers = match service.list_pipeline_triggers(pipeline_id).await {
            Ok(values) => values,
            Err(error) => return api_error(error.to_string()).into_response(),
        };
        for trigger in triggers
            .into_iter()
            .filter(|trigger| trigger.enabled && trigger.kind == WorkflowTriggerKind::Cron)
        {
            let Some(trigger_id) = trigger.id else {
                continue;
            };
            let (schedule, exclusions) = match calendar_schedules(
                &trigger.configuration,
                trigger.blackout_start.zip(trigger.blackout_end),
            ) {
                Ok(value) => value,
                Err(error) => return api_error(error).into_response(),
            };
            entries.push(runinator_scheduling::ical::CalendarEntry {
                uid: format!("pipeline-trigger-{trigger_id}"),
                summary: format!("Runinator pipeline · {}", pipeline.name),
                description: format!(
                    "Scheduled pipeline {}",
                    qualified_name(pipeline.namespace.as_deref(), &pipeline.name)
                ),
                schedule,
                exclusions,
            });
        }
    }
    let name = match query.scope {
        CalendarScope::Platform => "Runinator platform schedules".to_string(),
        CalendarScope::Organization => "Runinator organization schedules".to_string(),
        CalendarScope::User => "My Runinator schedules".to_string(),
    };
    match runinator_scheduling::ical::render(
        &name,
        &entries,
        chrono::Utc::now(),
        query.horizon_days.unwrap_or(180),
    ) {
        Ok(calendar) => (
            [
                (header::CONTENT_TYPE, "text/calendar; charset=utf-8"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=runinator-schedules.ics",
                ),
                (header::CACHE_CONTROL, "private, max-age=300"),
            ],
            calendar,
        )
            .into_response(),
        Err(error) => api_error(error.to_string()).into_response(),
    }
}

fn qualified_name(namespace: Option<&str>, name: &str) -> String {
    namespace
        .filter(|value| !value.is_empty())
        .map(|value| format!("{value}.{name}"))
        .unwrap_or_else(|| name.to_string())
}

fn calendar_schedules(
    configuration: &runinator_models::value::Value,
    legacy_blackout: Option<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>)>,
) -> Result<(ScheduleSpec, Vec<ScheduleSpec>), String> {
    let schedule = if let Some(value) = configuration.get("schedule") {
        serde_json::from_value(value.clone().into()).map_err(|error| error.to_string())?
    } else {
        let expression = configuration
            .get("cron")
            .and_then(runinator_models::value::Value::as_str)
            .ok_or_else(|| "scheduled trigger is missing schedule".to_string())?;
        ScheduleSpec {
            recurrence: ScheduleRecurrence::Cron {
                expression: expression.to_string(),
            },
            timezone: "UTC".to_string(),
            duration_seconds: 0,
        }
    };
    let mut exclusions: Vec<ScheduleSpec> = configuration
        .get("exclusions")
        .map(|value| serde_json::from_value(value.clone().into()))
        .transpose()
        .map_err(|error| error.to_string())?
        .unwrap_or_default();
    if let Some((start, end)) = legacy_blackout {
        exclusions.push(ScheduleSpec::once(start, end));
    }
    Ok((schedule, exclusions))
}

pub async fn list_freeze_windows<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<FreezeWindowsQuery>,
) -> Reply {
    let scope = query
        .org_id
        .and_then(|id| {
            runinator_models::rbac::ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(id),
            )
        })
        .unwrap_or(runinator_models::rbac::ScopeRef::PLATFORM);
    if let Err(reply) = ctx.require_scope_action(runinator_models::rbac::Action::View, scope) {
        return reply;
    }
    let windows = service
        .list_freeze_windows(query.org_id, query.active.unwrap_or(false))
        .await;
    match windows {
        Ok(windows) => (StatusCode::OK, Json(ApiResponse::FreezeWindowList(windows))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_freeze_window<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(mut window): ValidatedJson<NewFreezeWindow>,
) -> Reply {
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &window, Permission::Edit).await {
        return reply;
    }
    if let Some(workflow_id) = window.workflow_id {
        window.org_id = match service.workflow(workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id,
            Ok(None) => return not_found(format!("Workflow {workflow_id} not found")),
            Err(err) => return api_error(err.to_string()),
        };
    }
    match service.create_freeze_window(&window).await {
        Ok(window) => (StatusCode::CREATED, Json(ApiResponse::FreezeWindow(window))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_freeze_window<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
    ValidatedJson(mut window): ValidatedJson<NewFreezeWindow>,
) -> Reply {
    let current = match service.fetch_freeze_window(window_id).await {
        Ok(Some(window)) => window,
        Ok(None) => return not_found(format!("Freeze window {window_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let current_target = NewFreezeWindow {
        org_id: current.org_id,
        workflow_id: current.workflow_id,
        name: current.name,
        reason: current.reason,
        starts_at: current.starts_at,
        ends_at: current.ends_at,
        schedule: current.schedule,
        enabled: current.enabled,
    };
    if let Err(reply) =
        require_window_target(db.as_ref(), &ctx, &current_target, Permission::Edit).await
    {
        return reply;
    }
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &window, Permission::Edit).await {
        return reply;
    }
    if let Some(workflow_id) = window.workflow_id {
        window.org_id = match service.workflow(workflow_id).await {
            Ok(Some(workflow)) => workflow.org_id,
            Ok(None) => return not_found(format!("Workflow {workflow_id} not found")),
            Err(err) => return api_error(err.to_string()),
        };
    }
    match service.update_freeze_window(window_id, &window).await {
        Ok(Some(window)) => (StatusCode::OK, Json(ApiResponse::FreezeWindow(window))),
        Ok(None) => not_found(format!("Freeze window {window_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_freeze_window<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(window_id): Path<Uuid>,
) -> Reply {
    let current = match service.fetch_freeze_window(window_id).await {
        Ok(Some(window)) => window,
        Ok(None) => return not_found(format!("Freeze window {window_id} not found")),
        Err(err) => return api_error(err.to_string()),
    };
    let target = NewFreezeWindow {
        org_id: current.org_id,
        workflow_id: current.workflow_id,
        name: current.name,
        reason: current.reason,
        starts_at: current.starts_at,
        ends_at: current.ends_at,
        schedule: current.schedule,
        enabled: current.enabled,
    };
    if let Err(reply) = require_window_target(db.as_ref(), &ctx, &target, Permission::Edit).await {
        return reply;
    }
    match service.delete_freeze_window(window_id).await {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::TaskResponse(response))),
        Err(err) => api_error(err.to_string()),
    }
}

/// replay a cron trigger's slots across a past range. slots the loop already fired keep their
/// original run, so re-issuing an overlapping backfill is safe.
pub async fn backfill_workflow_trigger<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<SchedulingOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(trigger_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<BackfillRequest>,
) -> Reply {
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_trigger_workflow(trigger_id, Permission::Edit)
        .await
    {
        return reply;
    }
    if let Err(err) = service.validate_backfill(&request) {
        return api_error(err.to_string());
    }
    match service
        .backfill_workflow_trigger(trigger_id, &request)
        .await
    {
        Ok(response) => (StatusCode::OK, Json(ApiResponse::Backfill(response))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn require_window_target<
    T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore,
>(
    db: &T,
    ctx: &AuthContext,
    window: &NewFreezeWindow,
    needed: Permission,
) -> Result<(), (StatusCode, Json<ApiResponse>)> {
    if let Some(workflow_id) = window.workflow_id {
        return AuthzChecker::new(db, ctx)
            .require_workflow(workflow_id, needed)
            .await;
    }
    let scope = window
        .org_id
        .and_then(|id| {
            runinator_models::rbac::ScopeRef::new(
                runinator_models::rbac::ScopeKind::Organization,
                Some(id),
            )
        })
        .unwrap_or(runinator_models::rbac::ScopeRef::PLATFORM);
    ctx.require_scope_action(runinator_models::rbac::Action::SchedulesManage, scope)
}

/// the `schedules` endpoints.
pub fn routes<T: AuthorizationStore + RuntimeStore + DefinitionStore + ScheduleStore>(
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, patch, post};
    axum::Router::new()
        .route(
            "/freeze_windows",
            get(list_freeze_windows::<T>)
                .post(create_freeze_window::<T>)
                .layer(Extension(pool.clone())),
        )
        .route("/schedules/calendar.ics", get(schedule_calendar::<T>))
        .route(
            "/schedules/calendar-subscriptions",
            post(create_calendar_subscription::<T>),
        )
        .route(
            "/schedules/calendar-subscriptions/{id}",
            axum::routing::delete(delete_calendar_subscription::<T>),
        )
        .route(
            "/calendar/{token}/runinator.ics",
            get(subscribed_calendar::<T>),
        )
        .route(
            "/freeze_windows/{id}",
            patch(update_freeze_window::<T>)
                .delete(delete_freeze_window::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}/backfill",
            post(backfill_workflow_trigger::<T>).layer(Extension(pool.clone())),
        )
}
