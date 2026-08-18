//! taking the process's own stdout and stderr away from the protocol channel.
//!
//! the mcp server answers on stdout, and every command module prints with plain `println!` — so a
//! single `workflows list` would write a table into the middle of a json-rpc frame and desynchronise
//! the client. the redirection is therefore installed *under* the command modules, at the standard
//! stream itself, and `install` hands back a duplicate of the original stdout for the protocol to
//! answer on. this is the same arrangement the console uses, for the same reason.
//!
//! it is not `tui::capture`, and the difference is the sync point. the console streams into a
//! line-limited scrollback on a reader thread and never has to say "this command is finished"; a
//! tool result does, exactly, with nothing of the next command's output in it and nothing of this
//! one's missing. so the streams are pointed at a scratch file rather than at a pipe: a flush is
//! the sync point, the read is ordinary file i/o, and there is no reader thread to race.
//!
//! how a standard stream is moved is the only per-platform part, and it is the whole of
//! `capture/unix.rs` and `capture/windows.rs`: `dup2` on a descriptor there, `SetStdHandle` on a
//! console handle here. everything above that line — the scratch file, the read-and-discard, the
//! rewind — is the same on both, which is why it lives in this file and not in either of them.

use std::fs::File;
use std::path::PathBuf;

use crate::commands::Result;

#[cfg(unix)]
#[path = "capture/unix.rs"]
mod platform;

#[cfg(windows)]
#[path = "capture/windows.rs"]
mod platform;

#[cfg(not(any(unix, windows)))]
#[path = "capture/unsupported.rs"]
mod platform;

/// how much output is retained before the scratch file is rewound.
///
/// output is read and discarded per command, so this only bounds what a single runaway command can
/// leave behind between two reads.
const SCRATCH_LIMIT: u64 = 64 * 1024 * 1024;

/// what the server holds while command output is being captured; dropping it puts the streams back.
pub(crate) struct OutputCapture {
    inner: Option<platform::Redirect>,
}

impl OutputCapture {
    /// redirect stdout and stderr into a scratch file, returning the real stdout to answer on.
    pub(crate) fn install() -> Result<(Self, File)> {
        let (redirect, screen) = platform::install()?;
        Ok((
            Self {
                inner: Some(redirect),
            },
            screen,
        ))
    }

    /// everything written since the last call.
    pub(crate) fn take(&mut self) -> String {
        match self.inner.as_mut() {
            Some(redirect) => redirect.take(),
            None => String::new(),
        }
    }

    /// put the standard streams back.
    pub(crate) fn restore(&mut self) {
        if let Some(redirect) = self.inner.take() {
            redirect.restore();
        }
    }
}

impl Drop for OutputCapture {
    fn drop(&mut self) {
        self.restore();
    }
}

/// where the scratch file goes.
///
/// named for the process and the moment, because two servers under one client share a temp
/// directory and neither should be reading the other's output.
fn scratch_path() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("runinator-mcp-{}-{unique}.out", std::process::id()))
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
