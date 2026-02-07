use std::{
    collections::VecDeque,
    env, fs, io,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};

use crate::{
    config::{Paths, ProcessConfig, SupervisorConfig, resolve_path},
    display::{clear_screen, render_snapshot},
    os::{is_process_running, send_kill, send_terminate},
    snapshot::{ProcessSnapshot, StateSnapshot, write_snapshot},
    types::DynError,
};

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

pub fn start_daemon(paths: &Paths) -> Result<(), DynError> {
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
    let stdout = fs::OpenOptions::new()
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

pub fn run_supervisor(
    config: &SupervisorConfig,
    paths: &Paths,
    foreground: bool,
) -> Result<(), DynError> {
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

pub fn stop_supervisor(config: &SupervisorConfig, paths: &Paths) -> Result<(), DynError> {
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

    let stdout = fs::OpenOptions::new()
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
