//! the blob service binary.

use runinator_blob::{config::BlobServerConfig, run_server};
use runinator_utilities::startup::ProcessResources;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let process = ProcessResources::start("runinator-blob")?;
    let config = BlobServerConfig::from_env()?;
    run_server(config, async move { process.shutdown().cancelled().await }).await?;
    Ok(())
}
