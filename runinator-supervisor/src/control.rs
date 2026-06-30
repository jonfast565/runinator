use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::{config::ProcessConfig, types::DynError};

// monotonic tiebreaker so two commands enqueued in the same nanosecond get distinct file names.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// a dynamic control request applied by the running supervisor loop. dropped into the control
/// directory as one json file per command and drained in file-name (roughly chronological) order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ControlCommand {
    /// register a new process (started immediately when its `autostart` is set).
    AddProcess { process: ProcessConfig },
    /// start a registered process that is currently stopped.
    StartProcess { name: String },
    /// terminate a running process without removing it.
    StopProcess { name: String },
    /// terminate and forget a process entirely.
    RemoveProcess { name: String },
}

/// write a control command into the queue atomically (temp file then rename).
pub fn enqueue(control_dir: &Path, command: &ControlCommand) -> Result<(), DynError> {
    fs::create_dir_all(control_dir)?;
    let id = next_id();
    let tmp = control_dir.join(format!(".{id}.json.tmp"));
    let final_path = control_dir.join(format!("{id}.json"));
    fs::write(&tmp, serde_json::to_vec(command)?)?;
    fs::rename(&tmp, &final_path)?;
    Ok(())
}

/// read, parse, and remove every queued command. malformed or unreadable files are removed and
/// skipped so a single bad entry cannot wedge the loop.
pub fn drain(control_dir: &Path) -> Vec<ControlCommand> {
    let mut entries: Vec<_> = match fs::read_dir(control_dir) {
        Ok(read) => read
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect(),
        Err(_) => return Vec::new(),
    };
    entries.sort();

    let mut commands = Vec::with_capacity(entries.len());
    for path in entries {
        match fs::read(&path).map_err(DynError::from).and_then(|bytes| {
            serde_json::from_slice::<ControlCommand>(&bytes).map_err(DynError::from)
        }) {
            Ok(command) => commands.push(command),
            Err(_) => { /* fall through to removal. */ }
        }
        let _ = fs::remove_file(&path);
    }
    commands
}

fn next_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    // zero-padded nanos keep lexical order aligned with chronological order.
    format!("{nanos:039}-{:08x}-{seq:08x}", std::process::id())
}
