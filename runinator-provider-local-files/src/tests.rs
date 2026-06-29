use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use runinator_models::json;
use runinator_models::runs::ProviderExecutionRequest;
use runinator_models::value::Value;
use runinator_plugin::cancel::CancellationToken;
use runinator_plugin::provider::Provider;

use crate::LocalProvider;
use crate::sandbox::{ALLOW_WRITE_ENV, ROOT_ENV};

// env vars are process-global; serialize every env-touching test behind one lock.
static ENV_LOCK: Mutex<()> = Mutex::new(());
static COUNTER: AtomicU64 = AtomicU64::new(0);

struct Sandbox {
    root: PathBuf,
    artifacts: PathBuf,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl Sandbox {
    fn new(allow_write: bool) -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let base = std::env::temp_dir().join(format!("localfs-test-{}-{}", std::process::id(), id));
        let root = base.join("root");
        let artifacts = base.join("artifacts");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&artifacts).unwrap();
        unsafe {
            std::env::set_var(ROOT_ENV, &root);
            if allow_write {
                std::env::set_var(ALLOW_WRITE_ENV, "1");
            } else {
                std::env::remove_var(ALLOW_WRITE_ENV);
            }
        }
        Self {
            root,
            artifacts,
            _guard: guard,
        }
    }

    fn request(&self, function: &str, parameters: Value) -> ProviderExecutionRequest {
        ProviderExecutionRequest {
            run_id: None,
            action_name: "local".into(),
            action_function: function.into(),
            parameters,
            timeout_secs: 30,
            artifact_dir: self.artifacts.to_string_lossy().into_owned(),
            events_jsonl_path: String::new(),
        }
    }

    fn call(&self, function: &str, parameters: Value) -> Value {
        let result = LocalProvider
            .execute_service(
                self.request(function, parameters),
                None,
                CancellationToken::new(),
            )
            .expect("action should succeed");
        result.output_json.expect("output json")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        unsafe {
            std::env::remove_var(ROOT_ENV);
            std::env::remove_var(ALLOW_WRITE_ENV);
        }
        let _ = std::fs::remove_dir_all(self.root.parent().unwrap());
    }
}

#[test]
fn write_then_read_round_trips_and_tags_location_local() {
    let sb = Sandbox::new(true);
    let written = sb.call(
        "write_file",
        json!({ "path": "notes/hello.txt", "content": "hi there" }),
    );
    assert_eq!(written["location"], json!("local"));
    assert_eq!(written["size_bytes"], json!(8));

    let read = sb.call("read_file", json!({ "path": "notes/hello.txt" }));
    assert_eq!(read["location"], json!("local"));
    assert_eq!(read["content"], json!("hi there"));
}

#[test]
fn read_file_captures_a_local_artifact() {
    let sb = Sandbox::new(true);
    std::fs::write(sb.root.join("data.txt"), b"payload").unwrap();

    let result = LocalProvider
        .execute_service(
            sb.request("read_file", json!({ "path": "data.txt" })),
            None,
            CancellationToken::new(),
        )
        .expect("read should succeed");

    assert_eq!(result.artifacts.len(), 1);
    let artifact = &result.artifacts[0];
    assert_eq!(artifact.name, "data.txt");
    assert_eq!(artifact.metadata["location"], json!("local"));
    // the captured copy lives in the run artifact dir on this machine.
    assert!(PathBuf::from(&artifact.uri).starts_with(&sb.artifacts));
}

#[test]
fn list_dir_returns_entries() {
    let sb = Sandbox::new(true);
    std::fs::write(sb.root.join("a.txt"), b"a").unwrap();
    std::fs::create_dir(sb.root.join("sub")).unwrap();

    let listed = sb.call("list_dir", json!({ "path": "." }));
    let entries = listed["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 2);
}

#[test]
fn stat_reports_missing_and_present() {
    let sb = Sandbox::new(true);
    let missing = sb.call("stat", json!({ "path": "nope.txt" }));
    assert_eq!(missing["exists"], json!(false));

    std::fs::write(sb.root.join("here.txt"), b"x").unwrap();
    let present = sb.call("stat", json!({ "path": "here.txt" }));
    assert_eq!(present["exists"], json!(true));
    assert_eq!(present["is_dir"], json!(false));
}

#[test]
fn rejects_parent_dir_escape() {
    let sb = Sandbox::new(true);
    let err = LocalProvider
        .execute_service(
            sb.request("read_file", json!({ "path": "../escape.txt" })),
            None,
            CancellationToken::new(),
        )
        .expect_err("parent-dir escape must be rejected");
    assert!(err.to_string().contains("LOCALFS003"));
}

#[test]
fn rejects_absolute_path_escape() {
    let sb = Sandbox::new(true);
    let err = LocalProvider
        .execute_service(
            sb.request("read_file", json!({ "path": "/etc/passwd" })),
            None,
            CancellationToken::new(),
        )
        .expect_err("absolute path must be rejected");
    assert!(err.to_string().contains("LOCALFS003"));
}

#[test]
fn write_is_disabled_without_the_flag() {
    let sb = Sandbox::new(false);
    let err = LocalProvider
        .execute_service(
            sb.request("write_file", json!({ "path": "x.txt", "content": "y" })),
            None,
            CancellationToken::new(),
        )
        .expect_err("writes must be disabled by default");
    assert!(err.to_string().contains("LOCALFS006"));
}
