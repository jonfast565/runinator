//! restart budgets, spawn failures, and forced shutdown through fake processes.
use super::*;
use std::sync::Mutex;

#[derive(Debug)]
struct FailingBackend;
impl ProcessBackend for FailingBackend {
    fn spawn(&self, _: &mut Command) -> io::Result<Box<dyn ManagedChild>> {
        Err(io::Error::other("fake spawn failure"))
    }
}
#[derive(Debug)]
struct FakeChild(Arc<Mutex<Vec<&'static str>>>);
impl ManagedChild for FakeChild {
    fn id(&self) -> u32 {
        42
    }
    fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
        Ok(None)
    }
    fn wait(&mut self) -> io::Result<ExitStatus> {
        self.0.lock().unwrap().push("wait");
        Err(io::Error::other("fake wait"))
    }
    fn terminate(&mut self) -> Result<(), DynError> {
        self.0.lock().unwrap().push("terminate");
        Ok(())
    }
    fn kill(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().push("kill");
        Ok(())
    }
}
fn process() -> ManagedProcess {
    let dir = std::env::temp_dir().join(format!(
        "runinator-supervisor-traits-{}-{}",
        std::process::id(),
        Utc::now().timestamp_nanos_opt().unwrap()
    ));
    fs::create_dir_all(&dir).unwrap();
    let paths = Paths {
        config_path: dir.clone(),
        config_dir: dir.clone(),
        state_dir: dir.clone(),
        pid_file: dir.clone(),
        stop_file: dir.clone(),
        state_file: dir.clone(),
        control_dir: dir.clone(),
        logs_dir: dir.clone(),
        supervisor_log: dir,
    };
    let config = serde_json::from_value(
        serde_json::json!({"name":"fake", "command":"missing", "max_restarts_per_minute":1}),
    )
    .unwrap();
    let mut process = build_one_process(&config, &paths);
    process.backend = Arc::new(FailingBackend);
    process
}
#[test]
fn spawn_failure_exhausts_restart_budget() {
    let mut process = process();
    attempt_start(&mut process, Duration::ZERO).unwrap();
    assert!(matches!(process.status, ProcStatus::Backoff));
    poll_process(&mut process, Instant::now(), Duration::ZERO).unwrap();
    assert!(matches!(process.status, ProcStatus::Failed));
    assert!(process.next_restart_at.is_none());
    assert_eq!(process.restarts, 1);
    fs::remove_dir_all(process.logs_dir).unwrap();
}
#[test]
fn shutdown_escalates_and_reaps() {
    let mut process = process();
    let calls = Arc::new(Mutex::new(Vec::new()));
    process.child = Some(Box::new(FakeChild(calls.clone())));
    stop_children(std::slice::from_mut(&mut process), Duration::ZERO).unwrap();
    assert_eq!(*calls.lock().unwrap(), ["terminate", "kill", "wait"]);
    assert!(process.child.is_none());
    assert!(matches!(process.status, ProcStatus::Stopped));
    fs::remove_dir_all(process.logs_dir).unwrap();
}
