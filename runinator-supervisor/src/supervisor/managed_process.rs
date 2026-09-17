#[allow(unused_imports)]
use super::*;

#[derive(Debug)]
pub(super) struct ManagedProcess {
    pub(super) config: ProcessConfig,
    pub(super) command_path: PathBuf,
    pub(super) cwd_path: PathBuf,
    pub(super) child: Option<Box<dyn ManagedChild>>,
    pub(super) backend: Arc<dyn ProcessBackend>,
    pub(super) status: ProcStatus,
    pub(super) started_at_utc: Option<DateTime<Utc>>,
    pub(super) started_instant: Option<Instant>,
    pub(super) restarts: u32,
    pub(super) last_exit_code: Option<i32>,
    pub(super) last_error: Option<String>,
    pub(super) next_restart_at: Option<Instant>,
    pub(super) restart_history: VecDeque<Instant>,
    pub(super) logs_dir: PathBuf,
    pub(super) log_path: PathBuf,
    pub(super) start_count: u32,
    // set when a control command stopped this process, so the poll loop does not auto-restart it.
    pub(super) manual_stop: bool,
}
