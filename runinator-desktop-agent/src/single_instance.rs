//! Single-instance guard for the desktop agent. The process registers one exclusive `desktop`
//! replica, so two copies would compete for the same work. Bind a fixed loopback TCP port; the OS
//! releases it when the process exits, including after a crash. No stale lock file is needed.

use std::io;
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener};

// fixed loopback port used solely as a single-instance mutex. picked well outside the ranges the
// Runinator services use ports such as WS=8080, so this port is unlikely to collide with them.
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

/// tell the operator a copy is already running, then let the caller exit. a headless launch reports
/// on stderr, where its supervisor will capture it; a windowed launch blocks on a modal on purpose,
/// since the user actively launched a second instance and the tray app has no visible stderr.
pub fn warn_already_running(headless: bool) {
    if headless {
        eprintln!("A Runinator Desktop Agent is already running on this machine; exiting.");
        return;
    }
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Info)
        .set_title("Runinator Desktop Agent")
        .set_description(
            "The Runinator Desktop Agent is already running. Open it from the menu-bar (tray) icon.",
        )
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}
