use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Mutex, MutexGuard, OnceLock},
    thread,
    time::Duration,
};

use runinator_models::runs::ProviderExecutionRequest;
use serde_json::{Value, json};

use crate::send::send_notification;

#[test]
fn notification_action_posts_notification_record() {
    let (service_url, request_handle) = spawn_notification_server(r#"{"id":99}"#);
    let _env = ServiceUrlEnvGuard::set(&service_url);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let request = ProviderExecutionRequest {
        run_id: Some(41),
        action_name: "email".into(),
        action_function: "notify".into(),
        parameters: json!({
            "title": "Build finished",
            "body": "Release build completed",
            "severity": "success",
            "target": "command-center",
            "metadata": { "workflow": "release" }
        }),
        timeout_secs: 30,
        artifact_dir: String::new(),
        events_jsonl_path: String::new(),
    };

    let result = runtime.block_on(send_notification(&request)).unwrap();
    assert_eq!(
        result.output_json.as_ref().unwrap()["notification_id"],
        json!(99)
    );

    let recorded = request_handle.join().unwrap();
    assert_eq!(recorded.path, "/notifications");

    let body: Value = serde_json::from_str(&recorded.body).unwrap();
    assert_eq!(body["workflow_run_id"], json!(41));
    assert_eq!(body["channel"], json!("in_app"));
    assert_eq!(body["severity"], json!("success"));
    assert_eq!(body["title"], json!("Build finished"));
    assert_eq!(body["body"], json!("Release build completed"));
    assert_eq!(body["target"], json!("command-center"));
    assert_eq!(body["metadata"]["workflow"], json!("release"));
}

struct RecordedRequest {
    path: String,
    body: String,
}

fn spawn_notification_server(
    response_body: &'static str,
) -> (String, thread::JoinHandle<RecordedRequest>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let recorded = read_http_request(&mut stream);
        let response = format!(
            "HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            response_body.len(),
            response_body
        );
        stream.write_all(response.as_bytes()).unwrap();
        recorded
    });
    (format!("http://{addr}"), handle)
}

fn read_http_request(stream: &mut TcpStream) -> RecordedRequest {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk).unwrap();
        assert_ne!(read, 0, "client closed before sending headers");
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };

    let headers = String::from_utf8_lossy(&buffer[..header_end]);
    let request_line = headers.lines().next().unwrap();
    let path = request_line.split_whitespace().nth(1).unwrap().to_string();
    let content_length = content_length(&headers);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = stream.read(&mut chunk).unwrap();
        assert_ne!(read, 0, "client closed before sending body");
        buffer.extend_from_slice(&chunk[..read]);
    }

    RecordedRequest {
        path,
        body: String::from_utf8(buffer[body_start..body_start + content_length].to_vec()).unwrap(),
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn content_length(headers: &str) -> usize {
    headers
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse().ok())
        .unwrap_or(0)
}

struct ServiceUrlEnvGuard {
    original: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl ServiceUrlEnvGuard {
    fn set(value: &str) -> Self {
        let guard = env_lock().lock().unwrap();
        let original = env::var_os("RUNINATOR_SERVICE_URL");
        // safety: this test serializes access to the process env inside this crate.
        unsafe {
            env::set_var("RUNINATOR_SERVICE_URL", value);
        }
        Self {
            original,
            _guard: guard,
        }
    }
}

impl Drop for ServiceUrlEnvGuard {
    fn drop(&mut self) {
        // safety: this test serializes access to the process env inside this crate.
        unsafe {
            match &self.original {
                Some(value) => env::set_var("RUNINATOR_SERVICE_URL", value),
                None => env::remove_var("RUNINATOR_SERVICE_URL"),
            }
        }
    }
}

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
