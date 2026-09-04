use super::*;

#[tauri::command]
pub async fn list_my_orgs(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "orgs/me").await
}

#[tauri::command]
pub async fn list_orgs(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "orgs").await
}

#[tauri::command]
pub async fn create_org(
    state: State<'_, CommandCenterState>,
    name: String,
) -> CommandResult<Value> {
    post_json(&state, "orgs", &serde_json::json!({ "name": name })).await
}

#[tauri::command]
pub async fn switch_org(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/switch-org",
        &serde_json::json!({ "org_id": org_id }),
    )
    .await
}

#[tauri::command]
pub async fn switch_platform(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    post_json(&state, "auth/switch-platform", &serde_json::json!({})).await
}

#[tauri::command]
pub async fn list_org_members(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("orgs/{org_id}/members")).await
}

#[tauri::command]
pub async fn add_org_member(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
    user_id: Uuid,
    role: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("orgs/{org_id}/members"),
        &serde_json::json!({ "user_id": user_id, "role": role }),
    )
    .await
}

#[tauri::command]
pub async fn update_org_member(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
    user_id: Uuid,
    role: String,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("orgs/{org_id}/members/{user_id}")).await?;
    let response = state
        .client
        .read()
        .await
        .patch(url.clone())
        .json(&serde_json::json!({ "role": role }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn remove_org_member(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
    user_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("orgs/{org_id}/members/{user_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn fetch_rate_card(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "rate-card").await
}

#[tauri::command]
pub async fn fetch_org_nodes(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("orgs/{org_id}/nodes")).await
}

#[tauri::command]
pub async fn scale_org_nodes(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
    request: Value,
) -> CommandResult<Value> {
    post_json(&state, &format!("orgs/{org_id}/nodes/scale"), &request).await
}

#[tauri::command]
pub async fn fetch_org_quota(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("orgs/{org_id}/quota")).await
}

#[tauri::command]
pub async fn fetch_org_usage(
    state: State<'_, CommandCenterState>,
    org_id: Uuid,
) -> CommandResult<Value> {
    get_json(&state, &format!("orgs/{org_id}/usage")).await
}

#[tauri::command]
pub async fn fetch_credentials(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<CredentialSummary>> {
    get_json(&state, "credentials").await
}

#[tauri::command]
pub async fn fetch_credential(
    state: State<'_, CommandCenterState>,
    scope: String,
    name: String,
    kind: Option<String>,
) -> CommandResult<Value> {
    let mut url = build_state_url(&state, "credentials").await?;
    url.query_pairs_mut()
        .append_pair("scope", &scope)
        .append_pair("name", &name)
        .append_pair("kind", kind.as_deref().unwrap_or("secret"));
    let response = state.client.read().await.get(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn save_credential(
    state: State<'_, CommandCenterState>,
    request: CredentialPutRequest,
) -> CommandResult<Value> {
    let url = build_state_url(&state, "credentials").await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&request)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_credential(
    state: State<'_, CommandCenterState>,
    scope: String,
    name: String,
    kind: Option<String>,
) -> CommandResult<Value> {
    let mut url = build_state_url(&state, "credentials").await?;
    url.query_pairs_mut()
        .append_pair("scope", &scope)
        .append_pair("name", &name)
        .append_pair("kind", kind.as_deref().unwrap_or("secret"));
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn move_credential(
    state: State<'_, CommandCenterState>,
    setting_id: uuid::Uuid,
    request: Value,
) -> CommandResult<Value> {
    let url = build_state_url(&state, &format!("credentials/{setting_id}")).await?;
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
