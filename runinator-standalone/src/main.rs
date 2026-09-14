mod config;
mod host;
mod provisioner;
mod runtime_factory;
mod state;

use std::{
    fs,
    io::{self, Write},
    path::Path,
    process::{Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

use clap::Parser;
use config::{Cli, Command, StandaloneConfig};
use runinator_models::local_runtime::LocalRuntimeSnapshot;
use state::StateTracker;

type DynError = Box<dyn std::error::Error + Send + Sync>;

fn main() -> Result<(), DynError> {
    let raw_args = std::env::args().collect::<Vec<_>>();
    if raw_args.get(1).is_some_and(|arg| {
        matches!(
            arg.as_str(),
            "--child-metadata"
                | "--child-handle"
                | "--child-poll"
                | "--child-validate"
                | "--poll-once"
        )
    }) {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime
            .block_on(runinator_adapter_host::run_child_command(&raw_args))
            .map_err(|error| io::Error::other(error.to_string()))?;
        return Ok(());
    }
    let cli = Cli::parse();
    let config = cli.resolved_config()?;
    match cli.command {
        Command::Start { foreground } => {
            if foreground {
                run_server(config)
            } else {
                start_daemon(&config)
            }
        }
        Command::Stop => stop(&config),
        Command::Restart { foreground } => {
            stop(&config)?;
            if foreground {
                run_server(config)
            } else {
                start_daemon(&config)
            }
        }
        Command::Status { watch } => show_status(&config, watch),
        Command::Logs {
            component,
            lines,
            watch,
        } => show_logs(&config, component.as_deref(), lines, watch),
        Command::Serve => run_server(config),
    }
}

fn run_server(config: StandaloneConfig) -> Result<(), DynError> {
    host::configure_environment(&config)?;
    tokio::runtime::Runtime::new()?.block_on(serve(config))
}

fn start_daemon(config: &StandaloneConfig) -> Result<(), DynError> {
    fs::create_dir_all(&config.state_dir)?;
    let pid_path = config.state_dir.join("standalone.pid");
    if let Some(pid) = read_pid(&pid_path)?
        && runinator_supervisor::os::is_process_running(pid)
    {
        println!("Runinator standalone is already running (PID {pid}).");
        return Ok(());
    }
    remove_if_exists(&config.state_dir.join("stop"))?;
    let config_path = config.state_dir.join("config.json");
    fs::write(&config_path, serde_json::to_vec_pretty(config)?)?;
    let log_path = config.state_dir.join("standalone.log");
    let stdout = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;
    let stderr = stdout.try_clone()?;
    let mut child = ProcessCommand::new(std::env::current_exe()?);
    runinator_supervisor::os::detach_daemon(&mut child);
    child
        .arg("--config-json")
        .arg(config_path)
        .arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    let spawned = child.spawn()?;
    let launcher_pid = spawned.id();
    drop(spawned);
    if !wait_for(
        || read_pid(&pid_path).ok().flatten().is_some(),
        Duration::from_secs(8),
    ) {
        return Err(io::Error::other(format!(
            "standalone did not start; check {}",
            log_path.display()
        ))
        .into());
    }
    println!(
        "Runinator standalone started (launcher PID {launcher_pid}). Logs: {}",
        log_path.display()
    );
    Ok(())
}

async fn serve(config: StandaloneConfig) -> Result<(), DynError> {
    fs::create_dir_all(&config.state_dir)?;
    let pid_path = config.state_dir.join("standalone.pid");
    if let Some(pid) = read_pid(&pid_path)?
        && pid != std::process::id()
        && runinator_supervisor::os::is_process_running(pid)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("standalone is already running with PID {pid}"),
        )
        .into());
    }
    fs::write(&pid_path, format!("{}\n", std::process::id()))?;
    remove_if_exists(&config.state_dir.join("stop"))?;
    let _telemetry = runinator_platform::startup::startup("Runinator Standalone")?;
    let tracker = StateTracker::new();
    let result = host::run(config.clone(), tracker).await;
    remove_if_exists(&pid_path)?;
    remove_if_exists(&config.state_dir.join("stop"))?;
    result.map_err(|error| error as DynError)
}

fn stop(config: &StandaloneConfig) -> Result<(), DynError> {
    let pid_path = config.state_dir.join("standalone.pid");
    let Some(pid) = read_pid(&pid_path)? else {
        println!("Runinator standalone is not running.");
        return Ok(());
    };
    fs::create_dir_all(&config.state_dir)?;
    fs::write(config.state_dir.join("stop"), b"stop\n")?;
    if wait_for(
        || !runinator_supervisor::os::is_process_running(pid),
        Duration::from_secs(20),
    ) {
        remove_if_exists(&pid_path)?;
        println!("Runinator standalone stopped.");
        return Ok(());
    }
    eprintln!("Graceful shutdown timed out. Attempting forced termination...");
    runinator_supervisor::os::send_terminate(pid)?;
    if !wait_for(
        || !runinator_supervisor::os::is_process_running(pid),
        Duration::from_secs(2),
    ) {
        runinator_supervisor::os::send_kill(pid)?;
    }
    remove_if_exists(&pid_path)?;
    Ok(())
}

fn show_status(config: &StandaloneConfig, watch: bool) -> Result<(), DynError> {
    loop {
        let path = config.state_dir.join("state.json");
        if !path.exists() {
            println!("No standalone state is available at {}.", path.display());
        } else {
            let snapshot: LocalRuntimeSnapshot = serde_json::from_slice(&fs::read(path)?)?;
            println!(
                "standalone pid={} updated={}",
                snapshot.pid, snapshot.updated_at
            );
            for component in snapshot.components {
                println!(
                    "{:<42} {:<14} {:<10} restarts={}",
                    component.id, component.kind, component.status, component.restarts
                );
            }
        }
        if !watch {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(1));
        print!("\x1b[2J\x1b[H");
        io::stdout().flush()?;
    }
}

fn show_logs(
    config: &StandaloneConfig,
    component: Option<&str>,
    lines: usize,
    watch: bool,
) -> Result<(), DynError> {
    let path = config.state_dir.join("standalone.log");
    let mut shown = 0usize;
    loop {
        let data = fs::read_to_string(&path).unwrap_or_default();
        let matching = data
            .lines()
            .filter(|line| component.is_none_or(|value| line.contains(value)))
            .collect::<Vec<_>>();
        for line in matching
            .iter()
            .skip(shown.max(matching.len().saturating_sub(lines)))
        {
            println!("{line}");
        }
        shown = matching.len();
        if !watch {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn read_pid(path: &Path) -> Result<Option<u32>, DynError> {
    if !path.exists() {
        return Ok(None);
    }
    Ok(fs::read_to_string(path)?.trim().parse().ok())
}

fn remove_if_exists(path: &Path) -> Result<(), DynError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn wait_for(mut predicate: impl FnMut() -> bool, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if predicate() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}
