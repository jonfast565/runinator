mod auth;
mod banner;
mod cli;
mod commands;
mod output;
mod params;
mod service;

use clap::Parser;

use crate::cli::{Cli, Commands, WorkflowCommands};
use service::CtlService;

#[tokio::main]
async fn main() -> commands::Result<()> {
    CtlService::new().run().await
}

async fn run_process() -> commands::Result<()> {
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
        // `workflows test` is a fully offline dry-run; run it without contacting the web service.
        Commands::Workflows {
            command:
                WorkflowCommands::Test {
                    file,
                    tests,
                    filter,
                },
        } => commands::workflows_test(file, tests, filter.as_deref(), cli.json),
        _ => {
            let client = auth::build_authenticated_client(&cli).await?;
            commands::run(&client, &cli).await
        }
    }
}
