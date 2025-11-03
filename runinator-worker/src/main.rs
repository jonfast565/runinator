mod config;
mod discovery;
mod executor;
mod provider_repository;
mod server;

use std::{env, sync::Arc, time::Duration};

use config::parse_config;
use discovery::DiscoveryService;
use log::{error, info};
use runinator_models::errors::SendableError;
use runinator_plugin::{load_libraries_from_path, print_libs};
use runinator_utilities::{dirutils, logger};

#[tokio::main]
async fn main() -> Result<(), SendableError> {
    unsafe {
        env::set_var("RUST_BACKTRACE", "1");
    }
    dirutils::set_exe_dir_as_cwd()?;
    logger::setup_logger()?;
    log_panics::init();

    info!("--- Runinator Worker ---");
    logger::print_env()?;

    let config = parse_config()?;
    info!("Worker ID: {}", config.worker_id);

    let libraries = load_libraries(&config.dll_path)?;
    let libraries = Arc::new(libraries);

    let discovery = DiscoveryService::new(&config)
        .await
        .map_err(|err| -> SendableError { Box::new(err) })?;
    discovery.start(Duration::from_secs(config.gossip_interval_seconds));

    let bind_address = config.command_bind.clone();
    let command_port = config.command_port;
    let server_libraries = Arc::clone(&libraries);
    let server_task = tokio::spawn(async move {
        if let Err(err) =
            server::run_command_server(&bind_address, command_port, server_libraries).await
        {
            error!("Command server terminated with error: {}", err);
        }
    });

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");
    info!("Shutdown signal received. Stopping worker...");
    server_task.abort();
    if let Err(err) = server_task.await {
        if !err.is_cancelled() {
            error!("Command server join error: {}", err);
        }
    }

    Ok(())
}

fn load_libraries(
    path: &str,
) -> Result<std::collections::HashMap<String, runinator_plugin::plugin::Plugin>, SendableError> {
    info!("Loading plugins from {}", path);
    let libraries = load_libraries_from_path(path)?;
    print_libs(&libraries);
    Ok(libraries)
}
