//! the agent's tracing bridge: installs a reloadable subscriber that renders `log`/`tracing` records
//! into the in-app log console, so an operator running the tray app (with no visible stderr) can read
//! broker/worker/routing detail. the GUI's log-level dropdown drives the filter live via
//! [`set_level`] — no restart, and `RUST_LOG` still wins at process startup for parity with a
//! terminal-launched worker.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::reload;

use crate::agent::{SharedHandle, try_log_line};
use crate::config::LogLevel;

// the on-disk agent log, under the shared app-data dir (`~/.runinator/logs/desktop-agent.log`). the
// tray app has no visible stderr, so a rolling console alone leaves no post-mortem trail; the file
// gives one for diagnosing routing/broker/worker issues after the fact.
const LOG_FILE_NAME: &str = "logs/desktop-agent.log";

// set once the subscriber is installed; lets the GUI change the filter without naming the (verbose)
// reload-handle type. a no-op before init, or if another subscriber was already installed.
static SET_LEVEL: OnceLock<Box<dyn Fn(LogLevel) + Send + Sync>> = OnceLock::new();

/// install the console-bridging subscriber with `initial` as its starting level (unless `RUST_LOG`
/// overrides it). idempotent-ish: a second call, or a pre-existing global subscriber, is ignored.
pub fn init(shared: SharedHandle, initial: LogLevel) {
    let (filter, handle) = reload::Layer::new(initial_filter(initial));
    let console_layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .with_target(true)
        .with_level(true)
        .without_time() // the console adds its own HH:MM:SS stamp.
        .with_writer(ConsoleMakeWriter {
            shared: shared.clone(),
        });

    // a persistent file sink alongside the console. its own EnvFilter (info default, `RUNINATOR_LOG`
    // honored) keeps the on-disk trail stable and diagnostic regardless of the live GUI level, and it
    // carries timestamps the console omits. best-effort: if the file cannot be opened, log to console
    // only rather than failing startup.
    let file_layer = open_log_file().map(|file| {
        tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_level(true)
            .with_writer(FileMakeWriter { file })
            .with_filter(file_filter(initial))
    });

    if tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .is_err()
    {
        return;
    }

    let _ = SET_LEVEL.set(Box::new(move |level| {
        let _ = handle.reload(directive_filter(level));
    }));
}

/// change the live log level from the GUI; a no-op until [`init`] has run.
pub fn set_level(level: LogLevel) {
    if let Some(apply) = SET_LEVEL.get() {
        apply(level);
    }
}

// the project-wide log-filter env var, matching the service binaries' shared logger.
const LOG_ENV: &str = "RUNINATOR_LOG";

/// open (creating dirs) the append-mode agent log file under the app-data dir. `None` on any io
/// error so logging degrades to console-only rather than blocking startup.
fn open_log_file() -> Option<Arc<Mutex<File>>> {
    let path: PathBuf = runinator_utilities::app_data::app_data_path(LOG_FILE_NAME).ok()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .ok()?;
    Some(Arc::new(Mutex::new(file)))
}

/// file-sink filter: honor `RUNINATOR_LOG`, else at least the persisted level, so the on-disk trail
/// is independent of the live GUI dropdown.
fn file_filter(level: LogLevel) -> EnvFilter {
    initial_filter(level)
}

/// startup filter: honor a `RUNINATOR_LOG` directive if present (parity with the service binaries'
/// shared logger), otherwise the persisted level.
fn initial_filter(level: LogLevel) -> EnvFilter {
    EnvFilter::try_from_env(LOG_ENV).unwrap_or_else(|_| directive_filter(level))
}

/// build a filter from a level, quieting the GUI/transport stacks at debug/trace so the console shows
/// runinator detail rather than a flood of egui/wgpu/http frames.
fn directive_filter(level: LogLevel) -> EnvFilter {
    let base = level.as_str();
    let directive = match level {
        LogLevel::Debug | LogLevel::Trace => format!(
            "{base},hyper=info,h2=info,reqwest=info,rustls=info,tungstenite=info,\
             tokio_tungstenite=info,tower=info,eframe=info,egui=info,winit=info,\
             wgpu_core=warn,wgpu_hal=warn,naga=warn"
        ),
        _ => base.to_string(),
    };
    EnvFilter::new(directive)
}

// a `MakeWriter` that feeds each formatted tracing line into the in-app log console.
struct ConsoleMakeWriter {
    shared: SharedHandle,
}

impl<'a> MakeWriter<'a> for ConsoleMakeWriter {
    type Writer = ConsoleWriter;

    fn make_writer(&'a self) -> Self::Writer {
        ConsoleWriter {
            shared: self.shared.clone(),
            buf: Vec::new(),
        }
    }
}

// buffers one event's bytes and flushes them as console lines on drop (the fmt layer creates a fresh
// writer per event and drops it once the line is written).
struct ConsoleWriter {
    shared: SharedHandle,
    buf: Vec<u8>,
}

impl Write for ConsoleWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for ConsoleWriter {
    fn drop(&mut self) {
        if self.buf.is_empty() {
            return;
        }
        let text = String::from_utf8_lossy(&self.buf);
        for line in text.lines() {
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                try_log_line(&self.shared, trimmed.to_string());
            }
        }
    }
}

// a `MakeWriter` that appends each formatted tracing line to the shared log file. the fmt layer
// writes a whole event per `make_writer`, so a single locked write per event keeps lines intact
// without interleaving across threads.
struct FileMakeWriter {
    file: Arc<Mutex<File>>,
}

impl<'a> MakeWriter<'a> for FileMakeWriter {
    type Writer = FileWriter;

    fn make_writer(&'a self) -> Self::Writer {
        FileWriter {
            file: self.file.clone(),
        }
    }
}

struct FileWriter {
    file: Arc<Mutex<File>>,
}

impl Write for FileWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        // a poisoned lock still yields the guard; a dropped file write is not worth panicking over.
        let mut file = match self.file.lock() {
            Ok(file) => file,
            Err(poisoned) => poisoned.into_inner(),
        };
        file.write_all(data)?;
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.file.lock() {
            Ok(mut file) => file.flush(),
            Err(poisoned) => poisoned.into_inner().flush(),
        }
    }
}
