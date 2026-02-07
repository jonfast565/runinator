use std::{
    io,
    process::{Command, Stdio},
};

use crate::types::DynError;

#[cfg(unix)]
pub fn is_process_running(pid: u32) -> bool {
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
pub fn is_process_running(pid: u32) -> bool {
    match Command::new("tasklist")
        .args(["/FI", &format!("PID eq {}", pid)])
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()),
        Err(_) => false,
    }
}

#[cfg(unix)]
pub fn send_terminate(pid: u32) -> Result<(), DynError> {
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
pub fn send_terminate(pid: u32) -> Result<(), DynError> {
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
pub fn send_kill(pid: u32) -> Result<(), DynError> {
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
pub fn send_kill(pid: u32) -> Result<(), DynError> {
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
