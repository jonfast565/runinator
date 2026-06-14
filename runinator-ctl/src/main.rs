mod auth;
mod banner;
mod cli;
mod commands;
mod output;
mod params;

use clap::Parser;

use crate::cli::{Cli, Commands};

#[tokio::main]
async fn main() -> commands::Result<()> {
    let cli = Cli::parse();
    // skip the banner in json mode to keep machine-readable output clean.
    if !cli.json {
        banner::print();
    }
    match &cli.command {
        Commands::Login { username, password } => {
            auth::login(&cli, username.clone(), password.clone()).await
        }
        Commands::Logout => auth::logout(&cli).await,
        _ => {
            let client = auth::build_authenticated_client(&cli).await?;
            commands::run(&client, &cli).await
        }
    }
}
