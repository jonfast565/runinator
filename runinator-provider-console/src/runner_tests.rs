use super::{allow_interactive, working_dir};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use runinator_models::{
    runs::{ProviderExecutionEvent, ProviderExecutionRequest},
    value::Value,
};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};

#[test]
fn interactive_gate_reads_env_flag() {
    // permitted only for a non-empty, non-"0" flag; unset/empty/"0" reject (cloud-worker default).
    assert!(allow_interactive(Some("1")));
    assert!(allow_interactive(Some("true")));
    assert!(!allow_interactive(Some("0")));
    assert!(!allow_interactive(Some("")));
    assert!(!allow_interactive(None));
}

#[test]
fn working_dir_reads_env_path() {
    // a non-empty, trimmed path is used; unset/empty/blank inherit the process cwd (None).
    assert_eq!(
        working_dir(Some("/tmp/work")),
        Some(PathBuf::from("/tmp/work"))
    );
    assert_eq!(
        working_dir(Some("  /tmp/work  ")),
        Some(PathBuf::from("/tmp/work"))
    );
    assert_eq!(working_dir(Some("")), None);
    assert_eq!(working_dir(Some("   ")), None);
    assert_eq!(working_dir(None), None);
}

#[derive(Default)]
struct RecordingSink(Mutex<Vec<ProviderExecutionEvent>>);

impl ProviderEventSink for RecordingSink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[cfg(unix)]
#[test]
fn noninteractive_commands_stream_stdout_and_stderr() {
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "console".into(),
        action_function: "run".into(),
        parameters: Value::from(serde_json::json!({
            "command": "printf 'ordinary\\n'; printf 'warning\\n' >&2"
        })),
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
    };
    let sink = Arc::new(RecordingSink::default());

    super::execute_command(
        &request,
        Some(sink.clone() as Arc<dyn ProviderEventSink>),
        CancellationToken::new(),
    )
    .unwrap();

    let events = sink.0.lock().unwrap();
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderExecutionEvent::Chunk { stream, content }
            if stream == "stdout" && content == "ordinary"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ProviderExecutionEvent::Chunk { stream, content }
            if stream == "stderr" && content == "warning"
    )));
}
