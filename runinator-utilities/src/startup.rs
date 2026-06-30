use crate::{
    dirutils,
    logger::{self, print_env},
    telemetry::TelemetryGuard,
};
use log::info;
use std::env;

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
    let guard = logger::setup_logger(name)?;
    log_panics::init();

    info!("--- {} ---", name);
    print_env()?;

    Ok(guard)
}
