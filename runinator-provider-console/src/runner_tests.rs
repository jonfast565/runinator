use super::{allow_interactive, working_dir};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc::Receiver},
    time::{Duration, Instant},
};

use runinator_models::{
    runs::{ProviderExecutionEvent, ProviderExecutionRequest, ProviderTerminalControl},
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

struct TerminalSink {
    events: Mutex<Vec<ProviderExecutionEvent>>,
    controls: Mutex<Option<Receiver<ProviderTerminalControl>>>,
}

impl ProviderEventSink for TerminalSink {
    fn emit(&self, event: ProviderExecutionEvent) {
        self.events.lock().unwrap().push(event);
    }

    fn take_terminal_control(&self) -> Option<Receiver<ProviderTerminalControl>> {
        self.controls.lock().unwrap().take()
    }
}

#[cfg(unix)]
#[test]
fn interactive_commands_receive_input_and_stream_terminal_bytes() {
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "console".into(),
        action_function: "run".into(),
        parameters: Value::Null,
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let sink = Arc::new(TerminalSink {
        events: Mutex::new(Vec::new()),
        controls: Mutex::new(Some(receiver)),
    });
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        sender
            .send(ProviderTerminalControl::Resize {
                cols: 100,
                rows: 30,
            })
            .unwrap();
        sender
            .send(ProviderTerminalControl::Input {
                data: "hello\r".into(),
            })
            .unwrap();
    });

    super::execute_interactive(
        &request,
        Some(sink.clone() as Arc<dyn ProviderEventSink>),
        CancellationToken::new(),
        "read value; printf 'got:%s\\n' \"$value\"".into(),
        Duration::from_secs(5),
        Instant::now(),
    )
    .unwrap();

    let terminal = sink
        .events
        .lock()
        .unwrap()
        .iter()
        .filter_map(|event| match event {
            ProviderExecutionEvent::Chunk { stream, content } if stream == "terminal" => {
                Some(content.as_str())
            }
            _ => None,
        })
        .collect::<String>();
    assert!(terminal.contains("got:hello"), "{terminal:?}");
}

#[test]
fn input_action_waits_for_a_line_and_emits_the_prompt_lifecycle() {
    let request = ProviderExecutionRequest {
        run_id: None,
        action_name: "console".into(),
        action_function: "input".into(),
        parameters: Value::from(serde_json::json!({ "prompt": "Your name?" })),
        timeout_secs: 5,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
        idempotency_key: None,
        workspace_path: None,
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    sender
        .send(ProviderTerminalControl::Resize { cols: 80, rows: 24 })
        .unwrap();
    sender
        .send(ProviderTerminalControl::Input {
            data: "Ada\r".into(),
        })
        .unwrap();
    let sink = Arc::new(TerminalSink {
        events: Mutex::new(Vec::new()),
        controls: Mutex::new(Some(receiver)),
    });

    let result = super::execute_input_with_permission(
        &request,
        Some(sink.clone() as Arc<dyn ProviderEventSink>),
        CancellationToken::new(),
        true,
    )
    .unwrap();

    assert_eq!(
        result.output_json,
        Some(Value::from(serde_json::json!({ "value": "Ada" })))
    );
    let events = sink.events.lock().unwrap();
    let interactions = events
        .iter()
        .filter_map(|event| match event {
            ProviderExecutionEvent::TerminalInteraction { interaction } => Some(interaction),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(interactions.len(), 2);
    assert_eq!(interactions[0].sequence, 1);
    assert_eq!(
        interactions[0].state,
        runinator_models::runs::TerminalInteractionState::InputRequired
    );
    assert_eq!(interactions[0].prompt.as_deref(), Some("Your name?"));
    assert_eq!(interactions[1].sequence, 2);
    assert_eq!(
        interactions[1].state,
        runinator_models::runs::TerminalInteractionState::InputAccepted
    );
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
