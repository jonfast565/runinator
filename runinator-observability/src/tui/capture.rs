//! Keep writes that bypass `tracing` from painting into the alternate screen.
//!
//! The process streams are redirected to a pipe while the dashboard is running. The dashboard
//! itself draws through a separate handle on the real terminal, and a reader thread turns direct
//! stdout/stderr writes into ordinary rolling-log entries. Moving the streams is platform-specific;
//! the pipe reader is not.

use std::fs::File;
use std::io::{self, Read};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

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
pub(super) struct Capture {
    redirect: Option<platform::Redirect>,
}

impl Capture {
    pub(super) fn install(dashboard: Arc<Dashboard>) -> io::Result<(Self, Screen)> {
        let (redirect, screen) = platform::install(dashboard)?;
        Ok((
            Self {
                redirect: Some(redirect),
            },
            screen,
        ))
    }

    /// Restore stdout/stderr and wait for the reader to finish draining their final writes.
    pub(super) fn restore(&mut self) {
        if let Some(redirect) = self.redirect.take() {
            redirect.restore();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.restore();
    }
}

/// Move bytes from the redirected streams into the dashboard's log pane.
fn spawn_reader(source: File, dashboard: Arc<Dashboard>) -> io::Result<JoinHandle<()>> {
    thread::Builder::new()
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
                    Err(_) => break,
                }
            }
            flush_line(&dashboard, &mut line);
        })
}

fn flush_line(dashboard: &Dashboard, line: &mut Vec<u8>) {
    if line.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(line);
    let text = sanitize(&text);
    line.clear();
    if !text.is_empty() {
        dashboard.log_line(text);
    }
}

// A log line is rendered as terminal text rather than a byte stream. Do not allow a direct writer
// to sneak an ANSI control sequence back through ratatui when we display the captured text.
fn sanitize(line: &str) -> String {
    let mut clean = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(character) = chars.next() {
        if character == '\x1b' {
            if chars.next() == Some('[') {
                // CSI is complete at its final byte (U+0040..U+007E). Discard a malformed
                // sequence too: none of its bytes are useful dashboard content.
                for candidate in chars.by_ref() {
                    if ('@'..='~').contains(&candidate) {
                        break;
                    }
                }
            }
            continue;
        }
        if character == '\t' {
            clean.push(' ');
        } else if !character.is_control() {
            clean.push(character);
        }
    }
    clean.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn strips_terminal_controls_from_captured_output() {
        assert_eq!(sanitize("\u{1b}[1;31mfailed\u{1b}[0m\t "), "failed");
    }
}
