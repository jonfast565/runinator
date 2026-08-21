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
    cursor_id: Option<Uuid>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/interrupts"),
        &json!({ "source": source, "payload": payload, "cursor_id": cursor_id }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_all_artifacts(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<RunArtifact>> {
    get_json(&state, "artifacts").await
}

#[derive(serde::Deserialize)]
pub struct ArtifactUploadRequest {
    pub run_id: Uuid,
    #[serde(default)]
    pub workflow_node_run_id: Option<Uuid>,
}

#[tauri::command]
pub async fn upload_artifact(
    state: State<'_, CommandCenterState>,
    app: AppHandle,
    request: ArtifactUploadRequest,
) -> CommandResult<RunArtifact> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog().file().pick_file(move |file_path| {
        let path = file_path.and_then(|fp| fp.into_path().ok());
        let _ = tx.send(path);
    });
    let path = rx
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?
        .ok_or_else(|| CommandError::Unexpected("upload canceled".into()))?;

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact")
        .to_string();
    let mime_type = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    let url = build_state_url(&state, "artifacts/upload").await?;
    let mut form = reqwest::multipart::Form::new()
        .text("run_id", request.run_id.to_string())
        .text("name", file_name.clone())
        .text("mime_type", mime_type);
    if let Some(node_run_id) = request.workflow_node_run_id {
        form = form.text("workflow_node_run_id", node_run_id.to_string());
    }
    form = form.part(
        "file",
        reqwest::multipart::Part::bytes(bytes).file_name(file_name),
    );

    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .multipart(form)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<RunArtifact>().await?)
}

#[derive(serde::Serialize)]
pub struct ArtifactDownloadResult {
    pub saved_to: Option<String>,
}

#[tauri::command]
pub async fn download_artifact(
    state: State<'_, CommandCenterState>,
    app: AppHandle,
    artifact_id: Uuid,
    default_name: String,
) -> CommandResult<ArtifactDownloadResult> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .save_file(move |file_path| {
            let path = file_path.and_then(|fp| fp.into_path().ok());
            let _ = tx.send(path);
        });
    let Some(target) = rx
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?
    else {
        return Ok(ArtifactDownloadResult { saved_to: None });
    };

    let url = build_state_url(&state, &format!("artifacts/{artifact_id}/download")).await?;
    let response = state.client.read().await.get(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    let bytes = response.bytes().await?;
    tokio::fs::write(&target, bytes)
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    Ok(ArtifactDownloadResult {
        saved_to: Some(target.to_string_lossy().into_owned()),
    })
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
