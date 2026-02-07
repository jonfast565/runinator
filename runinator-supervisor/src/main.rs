use std::{
    collections::{BTreeMap, VecDeque},
    env,
    error::Error,
    fs::{self, OpenOptions},
    io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Local, Utc};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

type DynError = Box<dyn Error + Send + Sync>;

#[derive(Debug, Parser)]
#[command(
    name = "runinator-supervisor",
    about = "Lightweight local process supervisor for Runinator services"
)]
struct Cli {
    #[arg(
        short,
        long,
        global = true,
        default_value = "runinator-supervisor.json"
    )]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Start the supervisor daemon.
    Start {
        /// Run in the foreground instead of daemon mode.
        #[arg(long, default_value_t = false)]
        foreground: bool,
    },
    /// Stop the supervisor and all managed child processes.
    Stop,
    /// Show a table of managed process state.
    Status {
        /// Refresh continuously.
        #[arg(long, default_value_t = false)]
        watch: bool,
    },
    #[command(hide = true)]
    Supervise {
        #[arg(long, default_value_t = false)]
        foreground: bool,
    },
}

#[derive(Debug, Deserialize)]
struct SupervisorConfig {
    #[serde(default = "default_state_dir")]
    state_dir: String,
    #[serde(default = "default_shutdown_timeout_secs")]
    shutdown_timeout_secs: u64,
    #[serde(default = "default_restart_delay_ms")]
    restart_delay_ms: u64,
    #[serde(default)]
    processes: Vec<ProcessConfig>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProcessConfig {
    name: String,
    command: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    autostart: bool,
    #[serde(default = "default_true")]
    restart_on_failure: bool,
    #[serde(default = "default_max_restarts_per_minute")]
    max_restarts_per_minute: u32,
}

#[derive(Debug)]
struct Paths {
    config_path: PathBuf,
    config_dir: PathBuf,
    state_dir: PathBuf,
    pid_file: PathBuf,
    stop_file: PathBuf,
    state_file: PathBuf,
    logs_dir: PathBuf,
    supervisor_log: PathBuf,
}

#[derive(Debug, Clone, Copy)]
enum ProcStatus {
    Starting,
    Running,
    Backoff,
    Exited,
    Failed,
    Stopping,
    Stopped,
}

impl ProcStatus {
    fn as_str(self) -> &'static str {
        match self {
            ProcStatus::Starting => "starting",
            ProcStatus::Running => "running",
            ProcStatus::Backoff => "backoff",
            ProcStatus::Exited => "exited",
            ProcStatus::Failed => "failed",
            ProcStatus::Stopping => "stopping",
            ProcStatus::Stopped => "stopped",
        }
    }
}

#[derive(Debug)]
struct ManagedProcess {
    config: ProcessConfig,
    command_path: PathBuf,
    cwd_path: PathBuf,
    child: Option<Child>,
    status: ProcStatus,
    started_at_utc: Option<DateTime<Utc>>,
    started_instant: Option<Instant>,
    restarts: u32,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
    next_restart_at: Option<Instant>,
    restart_history: VecDeque<Instant>,
    log_path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct StateSnapshot {
    supervisor_pid: u32,
    config_path: String,
    started_at: String,
    updated_at: String,
    processes: Vec<ProcessSnapshot>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ProcessSnapshot {
    name: String,
    status: String,
    pid: Option<u32>,
    restarts: u32,
    uptime_seconds: Option<u64>,
    last_exit_code: Option<i32>,
    last_error: Option<String>,
    started_at: Option<String>,
    command: String,
    cwd: String,
    log_file: String,
}

fn default_true() -> bool {
    true
}

fn default_state_dir() -> String {
    ".runinator-supervisor".to_string()
}

fn default_shutdown_timeout_secs() -> u64 {
    10
}

fn default_restart_delay_ms() -> u64 {
    2000
}

fn default_max_restarts_per_minute() -> u32 {
    10
}

fn main() -> Result<(), DynError> {
    let cli = Cli::parse();
    let (config, paths) = load_config(&cli.config)?;

    match cli.command {
        Commands::Start { foreground } => {
            if foreground {
                run_supervisor(&config, &paths, true)?;
            } else {
                start_daemon(&paths)?;
            }
        }
        Commands::Stop => stop_supervisor(&config, &paths)?,
        Commands::Status { watch } => show_status(&paths, watch)?,
        Commands::Supervise { foreground } => run_supervisor(&config, &paths, foreground)?,
    }

    Ok(())
}

fn load_config(path: &Path) -> Result<(SupervisorConfig, Paths), DynError> {
    let cwd = env::current_dir()?;
    let config_path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };
    let config_path = config_path.canonicalize().map_err(|err| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Unable to resolve config path {}: {err}", config_path.display()),
        )
    })?;
    let config_dir = config_path
        .parent()
        .ok_or_else(|| io::Error::other("Config path has no parent directory"))?
        .to_path_buf();
    let data = fs::read_to_string(&config_path)?;
    let config: SupervisorConfig = serde_json::from_str(&data).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid config JSON in {}: {err}", config_path.display()),
        )
    })?;

    if config.processes.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Config has no processes").into());
    }

    let state_dir = resolve_path(&config_dir, Path::new(&config.state_dir));
    let paths = Paths {
        config_path,
        config_dir,
        pid_file: state_dir.join("supervisor.pid"),
        stop_file: state_dir.join("stop"),
        state_file: state_dir.join("state.json"),
        logs_dir: state_dir.join("logs"),
        supervisor_log: state_dir.join("supervisor.log"),
        state_dir,
    };

    Ok((config, paths))
}

fn resolve_path(base_dir: &Path, raw: &Path) -> PathBuf {
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        base_dir.join(raw)
    }
}

fn start_daemon(paths: &Paths) -> Result<(), DynError> {
    fs::create_dir_all(&paths.state_dir)?;
    fs::create_dir_all(&paths.logs_dir)?;
    remove_file_if_exists(&paths.stop_file)?;

    if let Some(pid) = read_pid(&paths.pid_file)? {
        if is_process_running(pid) {
            println!(
                "Supervisor is already running (PID {}). Use `status` or `stop`.",
                pid
            );
            return Ok(());
        }
        remove_file_if_exists(&paths.pid_file)?;
    }

    let exe = env::current_exe()?;
    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.supervisor_log)?;
    let stderr = stdout.try_clone()?;

    let mut child = Command::new(exe);
    child
        .arg("--config")
        .arg(&paths.config_path)
        .arg("supervise")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    let daemon = child.spawn()?;
    let spawned_pid = daemon.id();
    drop(daemon);

    let started = wait_for_pid_file(&paths.pid_file, Duration::from_secs(5));
    if !started {
        return Err(io::Error::other(format!(
            "Supervisor did not start correctly. Check {}",
            paths.supervisor_log.display()
        ))
        .into());
    }

    println!(
        "Supervisor started (launcher PID {}). Logs: {}",
        spawned_pid,
        paths.supervisor_log.display()
    );
    Ok(())
}

fn wait_for_pid_file(pid_file: &Path, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if pid_file.exists() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn run_supervisor(config: &SupervisorConfig, paths: &Paths, foreground: bool) -> Result<(), DynError> {
    fs::create_dir_all(&paths.state_dir)?;
    fs::create_dir_all(&paths.logs_dir)?;
    remove_file_if_exists(&paths.stop_file)?;

    if let Some(pid) = read_pid(&paths.pid_file)? {
        if is_process_running(pid) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Supervisor already running with PID {}", pid),
            )
            .into());
        }
    }

    fs::write(&paths.pid_file, format!("{}\n", std::process::id()))?;

    let mut processes = build_processes(config, paths)?;
    let started_at = Utc::now();
    let restart_delay = Duration::from_millis(config.restart_delay_ms);

    for process in &mut processes {
        if process.config.autostart {
            attempt_start(process, restart_delay)?;
        } else {
            process.status = ProcStatus::Stopped;
        }
    }

    loop {
        let now = Instant::now();
        for process in &mut processes {
            poll_process(process, now, restart_delay)?;
        }

        let snapshot = build_snapshot(paths, started_at, &processes);
        write_snapshot(&paths.state_file, &snapshot)?;

        if foreground {
            clear_screen();
            render_snapshot(&snapshot);
            println!();
            println!(
                "Stop with: runinator-supervisor --config {} stop",
                paths.config_path.display()
            );
        }

        if paths.stop_file.exists() {
            break;
        }

        thread::sleep(Duration::from_millis(500));
    }

    stop_children(&mut processes, Duration::from_secs(config.shutdown_timeout_secs))?;

    let final_snapshot = build_snapshot(paths, started_at, &processes);
    write_snapshot(&paths.state_file, &final_snapshot)?;
    remove_file_if_exists(&paths.pid_file)?;
    remove_file_if_exists(&paths.stop_file)?;

    Ok(())
}

fn build_processes(config: &SupervisorConfig, paths: &Paths) -> Result<Vec<ManagedProcess>, DynError> {
    let mut processes = Vec::with_capacity(config.processes.len());
    for process in &config.processes {
        let command_raw = Path::new(&process.command);
        let command_path = if command_raw.components().count() > 1 || command_raw.is_absolute() {
            resolve_path(&paths.config_dir, command_raw)
        } else {
            command_raw.to_path_buf()
        };

        let cwd_path = match &process.cwd {
            Some(raw) => resolve_path(&paths.config_dir, Path::new(raw)),
            None => paths.config_dir.clone(),
        };

        let log_path = paths
            .logs_dir
            .join(format!("{}.log", sanitize_name(&process.name)));

        processes.push(ManagedProcess {
            config: process.clone(),
            command_path,
            cwd_path,
            child: None,
            status: ProcStatus::Stopped,
            started_at_utc: None,
            started_instant: None,
            restarts: 0,
            last_exit_code: None,
            last_error: None,
            next_restart_at: None,
            restart_history: VecDeque::new(),
            log_path,
        });
    }
    Ok(processes)
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "process".to_string()
    } else {
        out
    }
}

fn attempt_start(process: &mut ManagedProcess, restart_delay: Duration) -> Result<(), DynError> {
    process.status = ProcStatus::Starting;

    let stdout = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&process.log_path)?;
    let stderr = stdout.try_clone()?;

    let mut cmd = Command::new(&process.command_path);
    cmd.args(&process.config.args)
        .current_dir(&process.cwd_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));

    for (key, value) in &process.config.env {
        cmd.env(key, value);
    }

    match cmd.spawn() {
        Ok(child) => {
            process.started_at_utc = Some(Utc::now());
            process.started_instant = Some(Instant::now());
            process.last_error = None;
            process.last_exit_code = None;
            process.next_restart_at = None;
            process.status = ProcStatus::Running;
            process.child = Some(child);
        }
        Err(err) => {
            process.last_error = Some(format!(
                "Failed to start '{}': {}",
                process.command_path.display(),
                err
            ));
            process.status = ProcStatus::Failed;
            schedule_restart(process, restart_delay);
        }
    }

    Ok(())
}

fn poll_process(
    process: &mut ManagedProcess,
    now: Instant,
    restart_delay: Duration,
) -> Result<(), DynError> {
    if let Some(child) = process.child.as_mut() {
        if let Some(status) = child.try_wait()? {
            handle_exit(process, status, restart_delay);
        } else {
            process.status = ProcStatus::Running;
        }
    }

    if process.child.is_none() {
        if let Some(next_at) = process.next_restart_at {
            if now >= next_at {
                attempt_start(process, restart_delay)?;
            } else {
                process.status = ProcStatus::Backoff;
            }
        }
    }

    Ok(())
}

fn handle_exit(process: &mut ManagedProcess, status: ExitStatus, restart_delay: Duration) {
    process.child = None;
    process.started_at_utc = None;
    process.started_instant = None;
    process.last_exit_code = status.code();

    if status.success() {
        process.status = ProcStatus::Exited;
        process.next_restart_at = None;
        return;
    }

    process.status = ProcStatus::Failed;
    process.last_error = Some(match status.code() {
        Some(code) => format!("Exited with code {}", code),
        None => "Terminated by signal".to_string(),
    });
    if process.config.restart_on_failure {
        schedule_restart(process, restart_delay);
    }
}

fn schedule_restart(process: &mut ManagedProcess, restart_delay: Duration) {
    if !process.config.restart_on_failure {
        return;
    }

    let now = Instant::now();
    let window = Duration::from_secs(60);
    while let Some(ts) = process.restart_history.front() {
        if now.duration_since(*ts) > window {
            process.restart_history.pop_front();
        } else {
            break;
        }
    }

    if process.restart_history.len() as u32 >= process.config.max_restarts_per_minute {
        process.status = ProcStatus::Failed;
        process.next_restart_at = None;
        process.last_error = Some(format!(
            "Restart budget exceeded ({} restarts/minute)",
            process.config.max_restarts_per_minute
        ));
        return;
    }

    process.restart_history.push_back(now);
    process.restarts = process.restarts.saturating_add(1);
    process.next_restart_at = Some(now + restart_delay);
    process.status = ProcStatus::Backoff;
}

fn stop_children(processes: &mut [ManagedProcess], timeout: Duration) -> Result<(), DynError> {
    for process in processes.iter_mut() {
        if let Some(pid) = process.child.as_ref().map(Child::id) {
            process.status = ProcStatus::Stopping;
            send_terminate(pid)?;
        }
    }

    let start = Instant::now();
    while start.elapsed() < timeout {
        let mut all_stopped = true;
        for process in processes.iter_mut() {
            if let Some(child) = process.child.as_mut() {
                if let Some(status) = child.try_wait()? {
                    process.last_exit_code = status.code();
                    process.child = None;
                    process.status = ProcStatus::Stopped;
                } else {
                    all_stopped = false;
                }
            }
        }
        if all_stopped {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(100));
    }

    for process in processes.iter_mut() {
        if let Some(child) = process.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
            process.child = None;
            process.status = ProcStatus::Stopped;
            process.last_error = Some("Force-killed during shutdown timeout".to_string());
        }
    }

    Ok(())
}

fn build_snapshot(paths: &Paths, started_at: DateTime<Utc>, processes: &[ManagedProcess]) -> StateSnapshot {
    let updated_at = Utc::now();
    let process_snapshots = processes
        .iter()
        .map(|process| ProcessSnapshot {
            name: process.config.name.clone(),
            status: process.status.as_str().to_string(),
            pid: process.child.as_ref().map(Child::id),
            restarts: process.restarts,
            uptime_seconds: process.started_instant.map(|t| t.elapsed().as_secs()),
            last_exit_code: process.last_exit_code,
            last_error: process.last_error.clone(),
            started_at: process.started_at_utc.map(|ts| ts.to_rfc3339()),
            command: format_command(&process.command_path, &process.config.args),
            cwd: process.cwd_path.display().to_string(),
            log_file: process.log_path.display().to_string(),
        })
        .collect();

    StateSnapshot {
        supervisor_pid: std::process::id(),
        config_path: paths.config_path.display().to_string(),
        started_at: started_at.to_rfc3339(),
        updated_at: updated_at.to_rfc3339(),
        processes: process_snapshots,
    }
}

fn format_command(command: &Path, args: &[String]) -> String {
    let mut out = command.display().to_string();
    for arg in args {
        out.push(' ');
        out.push_str(arg);
    }
    out
}

fn write_snapshot(path: &Path, snapshot: &StateSnapshot) -> Result<(), DynError> {
    let temp = path.with_extension("json.tmp");
    let body = serde_json::to_vec_pretty(snapshot)?;
    fs::write(&temp, body)?;
    fs::rename(&temp, path)?;
    Ok(())
}

fn stop_supervisor(config: &SupervisorConfig, paths: &Paths) -> Result<(), DynError> {
    let pid = match read_pid(&paths.pid_file)? {
        Some(pid) => pid,
        None => {
            println!("Supervisor is not running.");
            return Ok(());
        }
    };

    fs::create_dir_all(&paths.state_dir)?;
    fs::write(&paths.stop_file, b"stop\n")?;

    let wait_timeout = Duration::from_secs(config.shutdown_timeout_secs + 2);
    if wait_for_process_exit(pid, wait_timeout) {
        remove_file_if_exists(&paths.pid_file)?;
        remove_file_if_exists(&paths.stop_file)?;
        println!("Supervisor stopped.");
        return Ok(());
    }

    eprintln!("Graceful shutdown timed out. Attempting forced termination...");
    send_terminate(pid)?;
    if !wait_for_process_exit(pid, Duration::from_secs(2)) {
        send_kill(pid)?;
    }
    remove_file_if_exists(&paths.pid_file)?;
    remove_file_if_exists(&paths.stop_file)?;
    println!("Supervisor stop requested.");
    Ok(())
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if !is_process_running(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    !is_process_running(pid)
}

fn show_status(paths: &Paths, watch: bool) -> Result<(), DynError> {
    loop {
        match read_snapshot(&paths.state_file) {
            Ok(snapshot) => {
                if watch {
                    clear_screen();
                }
                render_snapshot(&snapshot);
            }
            Err(err) => {
                if watch {
                    clear_screen();
                }
                println!("No supervisor state available: {}", err);
                if !watch {
                    return Ok(());
                }
            }
        }

        if !watch {
            return Ok(());
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn read_snapshot(path: &Path) -> Result<StateSnapshot, DynError> {
    let data = fs::read_to_string(path)?;
    let snapshot: StateSnapshot = serde_json::from_str(&data)?;
    Ok(snapshot)
}

fn render_snapshot(snapshot: &StateSnapshot) {
    println!("Runinator Supervisor");
    println!("PID: {}", snapshot.supervisor_pid);
    println!("Config: {}", snapshot.config_path);
    println!(
        "Started: {}",
        human_time(&snapshot.started_at).unwrap_or_else(|| snapshot.started_at.clone())
    );
    println!(
        "Updated: {}",
        human_time(&snapshot.updated_at).unwrap_or_else(|| snapshot.updated_at.clone())
    );
    println!();

    let headers = [
        "process",
        "status",
        "pid",
        "restarts",
        "uptime",
        "exit",
        "command",
    ];

    let mut rows = Vec::with_capacity(snapshot.processes.len());
    for process in &snapshot.processes {
        rows.push(vec![
            process.name.clone(),
            process.status.clone(),
            process.pid.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string()),
            process.restarts.to_string(),
            process
                .uptime_seconds
                .map(format_uptime)
                .unwrap_or_else(|| "-".to_string()),
            process
                .last_exit_code
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string()),
            truncate_cell(&process.command, 52),
        ]);
    }

    print_table(&headers, &rows);
}

fn truncate_cell(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }

    let mut out = String::new();
    for ch in value.chars().take(max_chars.saturating_sub(3)) {
        out.push(ch);
    }
    out.push_str("...");
    out
}

fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let mut widths: Vec<usize> = headers.iter().map(|v| v.len()).collect();
    for row in rows {
        for (idx, value) in row.iter().enumerate() {
            widths[idx] = widths[idx].max(value.chars().count());
        }
    }

    print_border('╔', '╦', '╗', &widths);
    print_row(
        &headers.iter().map(|v| (*v).to_string()).collect::<Vec<_>>(),
        &widths,
    );
    print_border('╠', '╬', '╣', &widths);
    for row in rows {
        print_row(row, &widths);
    }
    print_border('╚', '╩', '╝', &widths);
}

fn print_border(left: char, middle: char, right: char, widths: &[usize]) {
    print!("{}", left);
    for (idx, width) in widths.iter().enumerate() {
        print!("{}", "═".repeat(*width + 2));
        if idx + 1 == widths.len() {
            print!("{}", right);
        } else {
            print!("{}", middle);
        }
    }
    println!();
}

fn print_row(values: &[String], widths: &[usize]) {
    print!("║");
    for (idx, value) in values.iter().enumerate() {
        let width = widths[idx];
        let padding = width.saturating_sub(value.chars().count());
        print!(" {}{} ║", value, " ".repeat(padding));
    }
    println!();
}

fn format_uptime(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    format!("{hours:02}:{minutes:02}:{secs:02}")
}

fn clear_screen() {
    print!("\x1B[2J\x1B[H");
}

fn human_time(input: &str) -> Option<String> {
    let parsed = DateTime::parse_from_rfc3339(input).ok()?;
    let local = parsed.with_timezone(&Local);
    Some(local.format("%Y-%m-%d %H:%M:%S %Z").to_string())
}

fn read_pid(path: &Path) -> Result<Option<u32>, DynError> {
    if !path.exists() {
        return Ok(None);
    }

    let value = fs::read_to_string(path)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let pid = trimmed.parse::<u32>().map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid PID file {}: {err}", path.display()),
        )
    })?;
    Ok(Some(pid))
}

fn remove_file_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    match Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(windows)]
fn is_process_running(pid: u32) -> bool {
    match Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()),
        Err(_) => false,
    }
}

#[cfg(unix)]
fn send_terminate(pid: u32) -> Result<(), DynError> {
    if !is_process_running(pid) {
        return Ok(());
    }
    let status = Command::new("kill").arg(pid.to_string()).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Failed to terminate PID {}", pid)).into())
    }
}

#[cfg(windows)]
fn send_terminate(pid: u32) -> Result<(), DynError> {
    if !is_process_running(pid) {
        return Ok(());
    }
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Failed to terminate PID {}", pid)).into())
    }
}

#[cfg(unix)]
fn send_kill(pid: u32) -> Result<(), DynError> {
    if !is_process_running(pid) {
        return Ok(());
    }
    let status = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Failed to SIGKILL PID {}", pid)).into())
    }
}

#[cfg(windows)]
fn send_kill(pid: u32) -> Result<(), DynError> {
    if !is_process_running(pid) {
        return Ok(());
    }
    let status = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("Failed to force kill PID {}", pid)).into())
    }
}
