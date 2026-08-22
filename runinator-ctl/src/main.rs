mod auth;
mod banner;
mod cli;
mod commands;
mod output;
mod params;
mod service;
mod tui;

use clap::Parser;

use crate::cli::{Cli, Commands, FunctionCommands, WorkflowCommands};
use service::CtlService;

#[tokio::main]
async fn main() -> commands::Result<()> {
    CtlService::new().run().await
}

async fn run_process() -> commands::Result<()> {
    let cli = Cli::parse();
    // Skip the banner in JSON mode to keep machine-readable output clean. For the MCP server,
    // whose caller is a protocol client rather than a terminal; and for the console, which prints
    // it itself once its interface is up so that it lands at the top of the output pane instead of
    // on the screen the console is about to take over.
    if !cli.json && !matches!(cli.command, Commands::Mcp { .. } | Commands::Console { .. }) {
        banner::print();
    }
    match &cli.command {
        Commands::Login => auth::login(&cli).await,
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
        // `functions validate` archives and checks a package directory locally; no server needed.
        Commands::Functions {
            command: FunctionCommands::Validate { path },
        } => commands::functions_validate(path, cli.json),
        // The MCP server can start before the web service; see `build_client_or_offline`.
        Commands::Mcp { .. } => {
            let client = auth::build_client_or_offline(&cli).await?;
            commands::run(&client, &cli).await
        }
        _ => {
            let client = auth::build_authenticated_client(&cli).await?;
            commands::run(&client, &cli).await
        }
    }
}
