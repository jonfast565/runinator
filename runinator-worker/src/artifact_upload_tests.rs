//! covers which artifacts get relocated, and that a failed relocation leaves the original alone.
//!
//! the upload itself needs a web service, so these cover the decisions made before and after it —
//! which is where the behavior that matters lives: an artifact must never be silently lost or
//! rewritten to a uri whose bytes were not actually stored.

use super::*;
use runinator_comm::ActionCommand;
use runinator_models::json;
use runinator_models::workflows::WorkflowAction;
use uuid::Uuid;

fn command() -> ActionCommand {
    ActionCommand {
        command_id: Uuid::now_v7(),
        workflow_run_id: Uuid::now_v7(),
        workflow_node_run_id: Uuid::now_v7(),
        node_id: "node".into(),
        action: WorkflowAction {
            provider: "std".into(),
            function: "noop".into(),
            timeout_seconds: 60,
            configuration: Default::default(),
            mcp_enabled: false,
            tags: Vec::new(),
            required_labels: Default::default(),
            idempotency_key: None,
            function_binding: None,
        },
        attempt: 1,
        parameters: json!({}),
        target: Default::default(),
        trace_id: Uuid::now_v7(),
        trace_context: Default::default(),
        notification_delivery_id: None,
        invocation_call_id: None,
        idempotency_key: None,
    }
}

fn artifact(uri: &str) -> NewRunArtifact {
    NewRunArtifact {
        name: "report.txt".into(),
        mime_type: "text/plain".into(),
        size_bytes: 3,
        uri: uri.to_string(),
        metadata: json!({}),
    }
}

/// an uploader pointed at a port nothing listens on, so every upload attempt fails fast.
fn unreachable_uploader() -> Arc<ArtifactUploader> {
    let client = runinator_api::AsyncApiClient::new(runinator_api::StaticLocator::new(
        "http://127.0.0.1:1/",
    ))
    .expect("client builds");
    ArtifactUploader::new(client)
}

#[tokio::test]
async fn leaves_a_non_local_uri_alone() {
    let uploader = unreachable_uploader();
    // an already-durable uri is not ours to rewrite, and must not even be probed on disk.
    for uri in [
        "blob://runinator-run-artifacts/runs/a/b.txt",
        "https://example.com/report.txt",
        "relative/path.txt",
    ] {
        let mut item = artifact(uri);
        uploader.relocate(&command(), &mut item).await;
        assert_eq!(item.uri, uri);
    }
}

#[tokio::test]
async fn leaves_a_missing_file_alone() {
    let uploader = unreachable_uploader();
    let mut item = artifact("/no/such/artifact/file.txt");
    uploader.relocate(&command(), &mut item).await;
    assert_eq!(item.uri, "/no/such/artifact/file.txt");
}

#[tokio::test]
async fn keeps_the_local_path_when_the_upload_fails() {
    let dir = std::env::temp_dir().join(format!("runinator-upload-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let path = dir.join("report.txt");
    tokio::fs::write(&path, b"abc").await.unwrap();

    let uploader = unreachable_uploader();
    let mut item = artifact(&path.display().to_string());
    uploader.relocate(&command(), &mut item).await;

    // the web service is unreachable, so the artifact keeps the path it already had rather than
    // being rewritten to a uri whose bytes were never stored.
    assert_eq!(item.uri, path.display().to_string());
    let _ = tokio::fs::remove_dir_all(&dir).await;
}

#[tokio::test]
async fn refuses_to_buffer_an_oversized_artifact() {
    let dir = std::env::temp_dir().join(format!("runinator-upload-big-{}", Uuid::now_v7()));
    tokio::fs::create_dir_all(&dir).await.unwrap();
    let path = dir.join("big.bin");
    let file = tokio::fs::File::create(&path).await.unwrap();
    // sparse: sets the length without writing the bytes, so the size check is exercised cheaply.
    file.set_len(MAX_UPLOAD_BYTES + 1).await.unwrap();
    drop(file);

    let uploader = unreachable_uploader();
    let mut item = artifact(&path.display().to_string());
    uploader.relocate(&command(), &mut item).await;

    assert_eq!(item.uri, path.display().to_string());
    let _ = tokio::fs::remove_dir_all(&dir).await;
}
