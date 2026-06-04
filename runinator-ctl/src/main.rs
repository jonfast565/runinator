mod banner;
mod cli;
mod commands;
mod output;
mod pack;
mod params;

use clap::Parser;
use runinator_api::{AsyncApiClient, StaticLocator};

use crate::cli::Cli;

#[tokio::main]
async fn main() -> commands::Result<()> {
    let cli = Cli::parse();
    // skip the banner in json mode to keep machine-readable output clean.
    if !cli.json {
        banner::print();
    }
    let client = AsyncApiClient::new(StaticLocator::new(cli.api_base_url.clone()))?;
    commands::run(&client, &cli).await
}
