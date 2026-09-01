use super::*;
use runinator_models::schedules::{
    BackfillRequest, BackfillResponse, CalendarSubscriptionSecret, FreezeWindow, NewFreezeWindow,
};

#[tauri::command]
pub async fn fetch_freeze_windows(
    state: State<'_, CommandCenterState>,
    active_only: bool,
) -> CommandResult<Vec<FreezeWindow>> {
    get_json(
        &state,
        if active_only {
            "freeze_windows?active=true"
        } else {
            "freeze_windows"
        },
    )
    .await
}

#[tauri::command]
pub async fn create_freeze_window(
    state: State<'_, CommandCenterState>,
    window: NewFreezeWindow,
) -> CommandResult<FreezeWindow> {
    let value = post_json(&state, "freeze_windows", &json!(window)).await?;
    serde_json::from_value(value)
        .map_err(|error| CommandError::Unexpected(format!("invalid freeze window: {error}")))
}

#[tauri::command]
pub async fn update_freeze_window(
    state: State<'_, CommandCenterState>,
    window_id: Uuid,
    window: NewFreezeWindow,
) -> CommandResult<FreezeWindow> {
    let value = patch_json(
        &state,
        &format!("freeze_windows/{window_id}"),
        &json!(window),
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|error| CommandError::Unexpected(format!("invalid freeze window: {error}")))
}

#[tauri::command]
pub async fn delete_freeze_window(
    state: State<'_, CommandCenterState>,
    window_id: Uuid,
) -> CommandResult<TaskResponse> {
    delete(&state, &format!("freeze_windows/{window_id}")).await
}

#[tauri::command]
pub async fn backfill_workflow_trigger(
    state: State<'_, CommandCenterState>,
    trigger_id: Uuid,
    request: BackfillRequest,
) -> CommandResult<BackfillResponse> {
    let value = post_json(
        &state,
        &format!("workflow_triggers/{trigger_id}/backfill"),
        &json!(request),
    )
    .await?;
    serde_json::from_value(value)
        .map_err(|error| CommandError::Unexpected(format!("invalid backfill response: {error}")))
}

#[tauri::command]
pub async fn create_calendar_subscription(
    state: State<'_, CommandCenterState>,
    scope: String,
    org_id: Option<Uuid>,
) -> CommandResult<CalendarSubscriptionSecret> {
    let value = post_json(
        &state,
        "schedules/calendar-subscriptions",
        &json!({ "scope": scope, "org_id": org_id }),
    )
    .await?;
    serde_json::from_value(value).map_err(|error| {
        CommandError::Unexpected(format!("invalid calendar subscription response: {error}"))
    })
}

#[tauri::command]
pub async fn delete_calendar_subscription(
    state: State<'_, CommandCenterState>,
    subscription_id: Uuid,
) -> CommandResult<()> {
    let url = build_state_url(
        &state,
        &format!("schedules/calendar-subscriptions/{subscription_id}"),
    )
    .await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    handle_response(url, response).await?;
    Ok(())
}

#[tauri::command]
pub async fn download_schedule_calendar(
    state: State<'_, CommandCenterState>,
    scope: String,
    org_id: Option<Uuid>,
) -> CommandResult<Vec<u8>> {
    let mut path = format!("schedules/calendar.ics?scope={scope}");
    if let Some(org_id) = org_id {
        path.push_str(&format!("&org_id={org_id}"));
    }
    get_bytes(&state, &path).await
}
