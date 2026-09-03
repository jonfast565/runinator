use super::*;

#[tauri::command]
pub async fn auth_config(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "auth/config").await
}

#[tauri::command]
pub async fn auth_me(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "auth/me").await
}

#[tauri::command]
pub async fn update_current_user(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    patch_json(&state, "auth/me", &request).await
}

#[tauri::command]
pub async fn change_current_password(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "auth/me/password", &request).await
}

#[tauri::command]
pub async fn list_current_sessions(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "auth/sessions").await
}

#[tauri::command]
pub async fn revoke_current_session(
    state: State<'_, CommandCenterState>,
    session_id: Uuid,
) -> CommandResult<Value> {
    delete(&state, &format!("auth/sessions/{session_id}")).await
}

#[tauri::command]
pub async fn revoke_other_sessions(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    post_empty(&state, "auth/sessions/revoke-others").await
}

#[tauri::command]
pub async fn list_personal_api_keys(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "auth/me/api-keys").await
}

#[tauri::command]
pub async fn list_personal_api_key_scopes(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "auth/me/api-key-scopes").await
}

#[tauri::command]
pub async fn create_personal_api_key(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "auth/me/api-keys", &request).await
}

#[tauri::command]
pub async fn login(
    state: State<'_, CommandCenterState>,
    username: String,
    password: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/login",
        &json!({ "username": username, "password": password }),
    )
    .await
}

#[tauri::command]
pub async fn refresh_session(
    state: State<'_, CommandCenterState>,
    refresh_token: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/refresh",
        &json!({ "refresh_token": refresh_token }),
    )
    .await
}

#[tauri::command]
pub async fn logout(
    state: State<'_, CommandCenterState>,
    refresh_token: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/logout",
        &json!({ "refresh_token": refresh_token }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_auth_settings(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "auth/settings").await
}

#[tauri::command]
pub async fn save_auth_settings(
    state: State<'_, CommandCenterState>,
    max_refreshes: i64,
) -> CommandResult<Value> {
    put_json(
        &state,
        "auth/settings",
        &json!({ "max_refreshes": max_refreshes }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_server_settings(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "server/settings").await
}

#[tauri::command]
pub async fn save_server_settings(
    state: State<'_, CommandCenterState>,
    settings: Value,
) -> CommandResult<Value> {
    put_json(&state, "server/settings", &settings).await
}

/// store the access token so subsequent requests carry it.
#[tauri::command]
pub async fn set_access_token(
    state: State<'_, CommandCenterState>,
    token: Option<String>,
) -> CommandResult<()> {
    state.set_access_token(token).await;
    Ok(())
}

#[tauri::command]
pub async fn list_resource_grants(
    state: State<'_, CommandCenterState>,
    resource_type: String,
    resource_id: Uuid,
) -> CommandResult<Vec<Value>> {
    get_json(
        &state,
        &format!("authz/resources/{resource_type}/{resource_id}/grants"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_resource_owner(
    state: State<'_, CommandCenterState>,
    resource_type: String,
    resource_id: Uuid,
) -> CommandResult<Value> {
    get_json(
        &state,
        &format!("authz/resources/{resource_type}/{resource_id}/owner"),
    )
    .await
}

#[tauri::command]
pub async fn create_resource_grant(
    state: State<'_, CommandCenterState>,
    resource_type: String,
    resource_id: Uuid,
    principal_type: String,
    principal_id: Uuid,
    permission: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("authz/resources/{resource_type}/{resource_id}/grants"),
        &json!({
            "principal_type": principal_type,
            "principal_id": principal_id,
            "permission": permission,
        }),
    )
    .await
}

#[tauri::command]
pub async fn revoke_resource_grant(
    state: State<'_, CommandCenterState>,
    resource_type: String,
    resource_id: Uuid,
    grant_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(
        &state,
        &format!("authz/resources/{resource_type}/{resource_id}/grants/{grant_id}"),
    )
    .await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn transfer_resource_owner(
    state: State<'_, CommandCenterState>,
    resource_type: String,
    resource_id: Uuid,
    scope_kind: String,
    scope_id: Option<Uuid>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("authz/resources/{resource_type}/{resource_id}/owner"),
        &json!({ "owner": { "kind": scope_kind, "id": scope_id } }),
    )
    .await
}

#[tauri::command]
pub async fn delete_notification(
    state: State<'_, CommandCenterState>,
    notification_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("notifications/{notification_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_artifact(
    state: State<'_, CommandCenterState>,
    artifact_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("artifacts/{artifact_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_gate(
    state: State<'_, CommandCenterState>,
    gate_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("gates/{gate_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_automation_event(
    state: State<'_, CommandCenterState>,
    event_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("automation_events/{event_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn fetch_replica_providers(
    state: State<'_, CommandCenterState>,
    replica_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("replicas/{replica_id}/providers")).await
}

#[tauri::command]
pub async fn fetch_replica_samples(
    state: State<'_, CommandCenterState>,
    replica_id: Uuid,
    since_seconds: Option<i64>,
) -> CommandResult<Value> {
    let path = match since_seconds {
        Some(seconds) => format!("replicas/{replica_id}/samples?since_seconds={seconds}"),
        None => format!("replicas/{replica_id}/samples"),
    };
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn list_users(state: State<'_, CommandCenterState>) -> CommandResult<Vec<Value>> {
    get_json(&state, "users").await
}

#[tauri::command]
pub async fn create_user(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "users", &request).await
}

#[tauri::command]
pub async fn update_user(
    state: State<'_, CommandCenterState>,
    user_id: Uuid,
    request: Value,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("users/{user_id}")).await?;
    let response = state
        .client
        .read()
        .await
        .patch(url.clone())
        .json(&request)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_user(
    state: State<'_, CommandCenterState>,
    user_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("users/{user_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn list_teams(state: State<'_, CommandCenterState>) -> CommandResult<Vec<Value>> {
    get_json(&state, "teams").await
}

#[tauri::command]
pub async fn create_team(
    state: State<'_, CommandCenterState>,
    name: String,
) -> CommandResult<Value> {
    post_json(&state, "teams", &json!({ "name": name })).await
}

#[tauri::command]
pub async fn update_team(
    state: State<'_, CommandCenterState>,
    team_id: Uuid,
    name: String,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("teams/{team_id}")).await?;
    let response = state
        .client
        .read()
        .await
        .patch(url.clone())
        .json(&json!({ "name": name }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_team(
    state: State<'_, CommandCenterState>,
    team_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("teams/{team_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn list_team_members(
    state: State<'_, CommandCenterState>,
    team_id: Uuid,
) -> CommandResult<Vec<Value>> {
    get_json(&state, &format!("teams/{team_id}/members")).await
}

#[tauri::command]
pub async fn list_user_teams(
    state: State<'_, CommandCenterState>,
    user_id: Uuid,
) -> CommandResult<Vec<Value>> {
    get_json(&state, &format!("users/{user_id}/teams")).await
}

#[tauri::command]
pub async fn add_team_member(
    state: State<'_, CommandCenterState>,
    team_id: Uuid,
    user_id: Uuid,
    role: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("teams/{team_id}/members"),
        &json!({ "user_id": user_id, "role": role }),
    )
    .await
}

#[tauri::command]
pub async fn remove_team_member(
    state: State<'_, CommandCenterState>,
    team_id: Uuid,
    user_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("teams/{team_id}/members/{user_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn list_api_keys(state: State<'_, CommandCenterState>) -> CommandResult<Vec<Value>> {
    get_json(&state, "api_keys").await
}

#[tauri::command]
pub async fn create_api_key(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "api_keys", &request).await
}

#[tauri::command]
pub async fn update_api_key(
    state: State<'_, CommandCenterState>,
    key_id: Uuid,
    request: Value,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("api_keys/{key_id}")).await?;
    let response = state
        .client
        .read()
        .await
        .patch(url.clone())
        .json(&request)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn revoke_api_key(
    state: State<'_, CommandCenterState>,
    key_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("api_keys/{key_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn rotate_api_key(
    state: State<'_, CommandCenterState>,
    key_id: Uuid,
) -> CommandResult<Value> {
    post_json(&state, &format!("api_keys/{key_id}/rotate"), &json!({})).await
}

#[tauri::command]
pub async fn create_agent_enrollment_token(
    state: State<'_, CommandCenterState>,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, "agents/enrollment_tokens", &request).await
}

#[tauri::command]
pub async fn list_agent_enrollment_tokens(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "agents/enrollment_tokens").await
}

#[tauri::command]
pub async fn revoke_agent_enrollment_token(
    state: State<'_, CommandCenterState>,
    token_id: String,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("agents/enrollment_tokens/{token_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn list_agent_machines(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<Value>> {
    get_json(&state, "agents/machines").await
}

#[tauri::command]
pub async fn invalidate_agent_machine(
    state: State<'_, CommandCenterState>,
    machine_id: Uuid,
) -> CommandResult<Value> {
    delete(&state, &format!("agents/machines/{machine_id}")).await
}

#[tauri::command]
pub async fn list_dead_letters(
    state: State<'_, CommandCenterState>,
    channel: Option<String>,
    limit: Option<i64>,
) -> CommandResult<Vec<Value>> {
    let mut params: Vec<String> = Vec::new();
    if let Some(channel) = channel.filter(|c| !c.is_empty()) {
        params.push(format!("channel={channel}"));
    }
    if let Some(limit) = limit {
        params.push(format!("limit={limit}"));
    }
    let path = with_query("dead_letters", &params);
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn list_broker_messages(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Option<Uuid>,
    pipeline_run_id: Option<Uuid>,
    channel: Option<String>,
    limit: Option<i64>,
) -> CommandResult<Vec<Value>> {
    let mut params: Vec<String> = Vec::new();
    if let Some(workflow_run_id) = workflow_run_id {
        params.push(format!("workflow_run_id={workflow_run_id}"));
    }
    if let Some(pipeline_run_id) = pipeline_run_id {
        params.push(format!("pipeline_run_id={pipeline_run_id}"));
    }
    if let Some(channel) = channel.filter(|channel| !channel.is_empty()) {
        params.push(format!("channel={channel}"));
    }
    if let Some(limit) = limit {
        params.push(format!("limit={limit}"));
    }
    let path = with_query("broker_messages", &params);
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn list_audit_log(
    state: State<'_, CommandCenterState>,
    actor_id: Option<Uuid>,
    action: Option<String>,
    limit: Option<i64>,
) -> CommandResult<Vec<Value>> {
    let mut params: Vec<String> = Vec::new();
    if let Some(actor_id) = actor_id {
        params.push(format!("actor_id={actor_id}"));
    }
    if let Some(action) = action.filter(|a| !a.is_empty()) {
        params.push(format!("action={action}"));
    }
    if let Some(limit) = limit {
        params.push(format!("limit={limit}"));
    }
    let path = with_query("audit_log", &params);
    get_json(&state, &path).await
}
