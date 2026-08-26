use super::*;

#[tauri::command]
pub async fn fetch_approvals(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Option<Uuid>,
) -> CommandResult<Vec<Value>> {
    let path = match workflow_run_id {
        Some(run_id) => format!("approvals?workflow_run_id={run_id}"),
        None => "approvals".to_string(),
    };
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn deliver_signal(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    name: String,
    payload: Value,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/signals"),
        &json!({ "name": name, "payload": payload }),
    )
    .await
}

/// record an interrupt request on a run. the reducer decides whether it can be serviced.
#[tauri::command]
pub async fn request_run_interrupt(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    source: Option<String>,
    payload: Option<Value>,
    continuation_id: Option<Uuid>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/interrupts"),
        &json!({ "source": source, "payload": payload, "continuation_id": continuation_id }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_notifications(
    state: State<'_, CommandCenterState>,
    unread_only: bool,
    limit: i64,
) -> CommandResult<Vec<Value>> {
    let mut path = format!("notifications?limit={limit}");
    if unread_only {
        path.push_str("&unread=true");
    }
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn mark_notification_read(
    state: State<'_, CommandCenterState>,
    notification_id: Uuid,
) -> CommandResult<Value> {
    post_empty(
        &state,
        &format!("notifications/{notification_id}/mark_read"),
    )
    .await
}

#[tauri::command]
pub async fn mark_all_notifications_read(
    state: State<'_, CommandCenterState>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, "notifications/mark_all_read").await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}
