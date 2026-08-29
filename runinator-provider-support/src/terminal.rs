//! Cross-platform PTY/ConPTY execution with Runinator terminal controls.

use std::{
    fmt,
    io::{Read, Write},
    sync::{Arc, mpsc::Receiver},
    thread,
    time::{Duration, Instant},
};

pub use portable_pty::CommandBuilder;
use portable_pty::{PtySize, native_pty_system};
use runinator_models::runs::{ProviderExecutionEvent, ProviderTerminalControl};
use runinator_plugin::{cancel::CancellationToken, provider::ProviderEventSink};

pub const ALLOW_INTERACTIVE_ENV: &str = "RUNINATOR_CONSOLE_ALLOW_INTERACTIVE";

pub fn interactive_permitted() -> bool {
    matches!(std::env::var(ALLOW_INTERACTIVE_ENV).ok().as_deref(), Some(value) if !value.is_empty() && value != "0")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalExit {
    pub success: bool,
    pub exit_code: i32,
}

#[derive(Debug)]
pub enum TerminalError {
    Canceled,
    TimedOut(Duration),
    Unavailable(String),
}

impl fmt::Display for TerminalError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Canceled => write!(formatter, "terminal process canceled"),
            Self::TimedOut(timeout) => {
                write!(
                    formatter,
                    "terminal process timed out after {} seconds",
                    timeout.as_secs()
                )
            }
            Self::Unavailable(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for TerminalError {}

/// Run `command` in the platform's native pseudo-terminal. Output is emitted as raw `terminal`
/// chunks so an ANSI terminal emulator can reproduce cursor movement and color exactly.
pub fn run(
    command: CommandBuilder,
    sink: Option<Arc<dyn ProviderEventSink>>,
    token: CancellationToken,
    timeout: Duration,
) -> Result<TerminalExit, TerminalError> {
    let pty = native_pty_system();
    let pair = pty.openpty(PtySize::default()).map_err(unavailable)?;
    let mut child = pair.slave.spawn_command(command).map_err(unavailable)?;
    drop(pair.slave);

    let reader = pair.master.try_clone_reader().map_err(unavailable)?;
    let output = spawn_reader(reader, sink.clone());
    let controls = sink
        .as_ref()
        .and_then(|sink| sink.take_terminal_control())
        .unwrap_or_else(disconnected_receiver);
    let mut writer = Some(pair.master.take_writer().map_err(unavailable)?);
    let started = Instant::now();

    let status = loop {
        if token.is_cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            break Err(TerminalError::Canceled);
        }
        while let Ok(control) = controls.try_recv() {
            apply_control(pair.master.as_ref(), &mut writer, control)?;
        }
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TerminalError::TimedOut(timeout));
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(TerminalError::Unavailable(error.to_string()));
            }
        }
    };
    drop(writer);
    let _ = output.join();
    let status = status?;
    Ok(TerminalExit {
        success: status.success(),
        exit_code: i32::try_from(status.exit_code()).unwrap_or(-1),
    })
}

fn apply_control(
    master: &dyn portable_pty::MasterPty,
    writer: &mut Option<Box<dyn Write + Send>>,
    control: ProviderTerminalControl,
) -> Result<(), TerminalError> {
    match control {
        ProviderTerminalControl::Input { data } => {
            if let Some(writer) = writer.as_mut() {
                writer
                    .write_all(data.as_bytes())
                    .and_then(|()| writer.flush())
                    .map_err(|error| TerminalError::Unavailable(error.to_string()))?;
            }
        }
        ProviderTerminalControl::Resize { cols, rows } => master
            .resize(PtySize {
                cols: cols.max(1),
                rows: rows.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(unavailable)?,
        ProviderTerminalControl::Eof => {
            // A PTY is one bidirectional terminal, so dropping only our writer clone does not close
            // the master while the output reader is alive. Send the terminal EOT byte instead;
            // line disciplines and interactive CLIs interpret it exactly like Ctrl+D.
            if let Some(writer) = writer.as_mut() {
                writer
                    .write_all(&[0x04])
                    .and_then(|()| writer.flush())
                    .map_err(|error| TerminalError::Unavailable(error.to_string()))?;
            }
        }
    }
    Ok(())
}

fn spawn_reader(
    mut reader: Box<dyn Read + Send>,
    sink: Option<Arc<dyn ProviderEventSink>>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    if let Some(sink) = &sink {
                        sink.emit(ProviderExecutionEvent::Chunk {
                            stream: "terminal".into(),
                            content: String::from_utf8_lossy(&buffer[..read]).into_owned(),
                        });
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
    })
}

fn disconnected_receiver() -> Receiver<ProviderTerminalControl> {
    let (sender, receiver) = std::sync::mpsc::channel();
    drop(sender);
    receiver
}

fn unavailable(error: impl fmt::Display) -> TerminalError {
    TerminalError::Unavailable(error.to_string())
}
