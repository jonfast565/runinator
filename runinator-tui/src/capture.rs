//! Keep writes that bypass `tracing` from painting into the alternate screen.
//!
//! Shared by every full-screen Runinator terminal host.
//!
//! The process streams are redirected to a pipe while the dashboard is running. The dashboard
//! itself draws through a separate handle on the real terminal, and a reader thread turns direct
//! stdout/stderr writes into ordinary rolling-log entries. Moving the streams is platform-specific;
//! the pipe reader is not.

use std::fs::File;
use std::io::{self, Read};
use std::sync::Arc;
use std::{
    sync::mpsc::{self, Receiver},
    thread::{self, JoinHandle},
    time::Duration,
};

use super::Dashboard;

#[cfg(unix)]
#[path = "capture/unix.rs"]
mod platform;

#[cfg(windows)]
#[path = "capture/windows.rs"]
mod platform;

#[cfg(not(any(unix, windows)))]
#[path = "capture/unsupported.rs"]
mod platform;

/// The handle the dashboard uses to draw after stdout has been redirected.
pub(super) type Screen = File;

/// Process stream redirect held for the dashboard's lifetime.

/// Move bytes from the redirected streams into the dashboard's log pane.

fn spawn_reader(source: File, dashboard: Arc<Dashboard>) -> io::Result<Reader> {
    let (done_tx, done) = mpsc::sync_channel(1);
    let handle = thread::Builder::new()
        .name("runinator-tui-output".to_string())
        .spawn(move || {
            let mut source = source;
            let mut chunk = [0_u8; 8 * 1024];
            let mut line = Vec::new();
            loop {
                match source.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        for byte in &chunk[..read] {
                            match byte {
                                b'\n' | b'\r' => flush_line(&dashboard, &mut line),
                                byte => line.push(*byte),
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                    // Keep the read end alive across transient WSL pipe/PTY errors. Closing it
                    // while stdout still targets the pipe turns the next println into EPIPE.
                    Err(error) if error.kind() == io::ErrorKind::BrokenPipe => break,
                    Err(_) => thread::sleep(Duration::from_millis(10)),
                }
            }
            flush_line(&dashboard, &mut line);
            let _ = done_tx.send(());
        })?;
    Ok(Reader { handle, done })
}

fn flush_line(dashboard: &Dashboard, line: &mut Vec<u8>) {
    if line.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(line).into_owned();
    line.clear();
    if !text.trim().is_empty() {
        dashboard.log_line(text);
    }
}

mod capture;
pub(super) use capture::Capture;

mod reader;
use reader::Reader;
