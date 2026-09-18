mod auth;
mod banner;
mod commands;
mod output;
mod params;
mod service;

use clap::Parser;

use runinator_ctl_core::cli::{Cli, Commands, FunctionCommands, WorkflowCommands};
use service::CtlService;

#[tokio::main]
async fn main() -> commands::Result<()> {
    CtlService::new().run().await
}

async fn run_process() -> commands::Result<()> {
    let mut cli = Cli::parse();
    if full_screen_command(&cli.command) && !api_base_was_explicit() {
        let suggestion = auth::stored_server_suggestion()?;
        cli.api_base_url = runinator_tui::operations::select_server(suggestion.as_deref())?;
    }
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
        Commands::Workflows {
            command:
                WorkflowCommands::Scaffold {
                    path,
                    name,
                    namespace,
                },
        } => commands::workflows_scaffold(path, name.as_deref(), namespace, cli.json),
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
        // every `rexrap` verb is a local text transform over a file on disk; `workflows::rexrap`
        // takes no client at all. authenticating first meant a new operator could not syntax-check
        // a pack until a web service was running, and the failure named `auth/config` rather than
        // the missing server.
        Commands::RexRap { command } => commands::rexrap(command, cli.json),
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

fn full_screen_command(command: &Commands) -> bool {
    match command {
        Commands::Tui => true,
        Commands::Console {
            execute,
            file,
            plain,
            ..
        } => !plain && execute.is_none() && file.is_none(),
        _ => false,
    }
}

fn api_base_was_explicit() -> bool {
    api_base_was_explicit_in(
        std::env::var_os("RUNINATOR_API_BASE_URL"),
        std::env::args_os(),
    )
}

fn api_base_was_explicit_in(
    environment: Option<std::ffi::OsString>,
    arguments: impl IntoIterator<Item = std::ffi::OsString>,
) -> bool {
    if environment.is_some_and(|value| !value.is_empty()) {
        return true;
    }
    arguments.into_iter().any(|argument| {
        let argument = argument.to_string_lossy();
        argument == "--api-base-url" || argument.starts_with("--api-base-url=")
    })
}

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
