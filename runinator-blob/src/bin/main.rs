//! the blob service binary.

use runinator_blob::{config::BlobServerConfig, run_server};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let _telemetry = runinator_utilities::startup::startup("runinator-blob")?;
    let config = BlobServerConfig::from_env()?;
    run_server(config, shutdown()).await?;
    Ok(())
}

/// resolve on the first ctrl-c or sigterm, so a container stop drains in-flight requests instead of
/// dropping them.
async fn shutdown() {
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

    tokio::select! {
        _ = interrupt => {}
        _ = terminate => {}
    }
    tracing::info!("shutdown signal received; draining");
}
