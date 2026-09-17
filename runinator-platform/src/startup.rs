use crate::{app_data, dirutils};
use log::info;
use runinator_observability::{
    logger::{self, print_env},
    telemetry::TelemetryGuard,
};
use std::{
    env,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::Notify;

/// run the standard binary startup: set cwd, install logging + otel, init panic capture. returns
/// the telemetry guard, which the caller must keep alive for the process lifetime so otel signals
/// are flushed on shutdown.
pub fn startup(
    name: &str,
) -> Result<TelemetryGuard, Box<dyn std::error::Error + Send + Sync + 'static>> {
    unsafe {
        env::set_var("RUST_BACKTRACE", "1");
    }
    dirutils::set_exe_dir_as_cwd()?;
    let guard = logger::setup_logger(name, app_data::default_log_path().ok())?;
    log_panics::init();

    info!("--- {} ---", name);
    print_env()?;

    Ok(guard)
}

/// Process-scoped infrastructure shared by every long-running executable.
///
/// Keeping the telemetry guard here ensures it survives until process shutdown; consumers use the
/// contained [`Shutdown`] signal rather than installing their own signal listener.

async fn wait_for_signal() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = interrupt => {}, _ = terminate => {} }
}

#[cfg(test)]
mod tests {
    use super::Shutdown;

    #[tokio::test]
    async fn explicit_trigger_releases_all_waiters() {
        let shutdown = Shutdown::new();
        let first = shutdown.clone();
        let second = shutdown.clone();
        let a = tokio::spawn(async move { first.cancelled().await });
        let b = tokio::spawn(async move { second.cancelled().await });
        tokio::task::yield_now().await;
        shutdown.trigger();
        a.await.unwrap();
        b.await.unwrap();
    }
}

mod process_resources;
pub use process_resources::ProcessResources;

mod shutdown;
pub use shutdown::Shutdown;
