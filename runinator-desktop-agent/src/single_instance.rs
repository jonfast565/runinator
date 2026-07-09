//! single-instance guard for the desktop agent. this process registers the machine as the exclusive
//! `desktop` replica, so two copies running at once would both register that replica and contend for
//! the same pinned/labeled work — a race the operator never wants. we prevent it by binding a fixed
//! loopback tcp port at startup: the OS only lets one process hold it and releases it automatically
//! when that process exits (including on a crash), so there is no stale lock file to reap. the socket
//! is a pure liveness token — we never accept connections on it, we just hold it for the process's
//! life.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

// fixed loopback port used solely as a single-instance mutex. picked well outside the ranges the
// runinator services listen on (ws=8080, ...) and unlikely to collide with common local tooling.
const GUARD_PORT: u16 = 47_113;

/// held for the whole process lifetime; dropping it (on exit) frees the port for the next launch.
pub struct InstanceGuard {
    _listener: TcpListener,
}

/// try to become the sole running desktop agent. `Ok(Some(guard))` means we won the lock and must
/// keep the returned guard alive; `Ok(None)` means another instance already holds it; `Err` is an
/// unexpected bind failure that the caller should treat as inconclusive rather than a hard block.
pub fn acquire() -> io::Result<Option<InstanceGuard>> {
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, GUARD_PORT);
    match TcpListener::bind(addr) {
        Ok(listener) => Ok(Some(InstanceGuard {
            _listener: listener,
        })),
        Err(err) if err.kind() == io::ErrorKind::AddrInUse => Ok(None),
        Err(err) => Err(err),
    }
}

/// tell the operator a copy is already running, then let the caller exit. blocking on purpose: the
/// user actively launched a second instance, so surface a modal they can't miss (the tray app has no
/// visible stderr) before the process goes away.
pub fn warn_already_running() {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title("Runinator Desktop Agent")
        .set_description(
            "The Runinator Desktop Agent is already running. Open it from the menu-bar (tray) icon.",
        )
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
