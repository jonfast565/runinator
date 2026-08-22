//! running one `runinatorctl` command on behalf of a tool call.
//!
//! the line is handed to the same clap parser the process parses its own argv with, exactly as the
//! console does, so a command added to `Commands` is callable over MCP the day it is added — with
//! the same flags, defaults, and help. a second table of verbs here would be the copy nobody
//! updates.

use std::time::Duration;

use runinator_models::value::Value;

use super::capture::OutputCapture;
use super::protocol;
use crate::cli::{Commands, FunctionCommands, WorkflowCommands};
use crate::commands::{Client, repl, run_command};

/// a verb the server refuses to run, and what to do instead.
///
/// two kinds end up here: the ones that never return (an interactive screen, a watch loop) and the
/// ones that read the terminal. either would hang the tool call rather than answer it, so they are
/// refused with the alternative named rather than left to hit the timeout.
pub(super) struct Blocked {
    path: &'static [&'static str],
    reason: &'static str,
}

const BLOCKED: &[Blocked] = &[
    Blocked {
        path: &["console"],
        reason: "the console is an interactive screen that never returns. run its `:` commands \
                 directly instead — every one of them is a command line.",
    },
    Blocked {
        path: &["mcp"],
        reason: "that would start a second mcp server inside this one.",
    },
    Blocked {
        path: &["login"],
        reason: "login reads the terminal. set RUNINATOR_API_KEY (or run `runinatorctl login` in a \
                 shell) and restart the server.",
    },
    Blocked {
        path: &["logout"],
        reason: "logout changes the session this server is holding. run it in a shell instead.",
    },
    Blocked {
        path: &["workflows", "dev"],
        reason: "`workflows dev` watches for changes until interrupted. use `workflows apply` and \
                 then `workflows run`.",
    },
    Blocked {
        path: &["runs", "watch"],
        reason: "`runs watch` refreshes until interrupted. use `runs show`, or poll it.",
    },
];

/// run one command line and return the tool result it produced.
pub(crate) async fn exec(
    client: &Client,
    capture: &mut OutputCapture,
    line: &str,
    json: bool,
    timeout: Duration,
    api_base_url: &str,
) -> Value {
    let tokens = match repl::tokenize(line) {
        Ok(tokens) => tokens,
        Err(failure) => return protocol::text_result(failure.to_string(), true),
    };
    if tokens.is_empty() {
        return protocol::text_result("nothing to run: `command` is empty", true);
    }
    run(client, capture, tokens, json, timeout, api_base_url, line).await
}

/// run an argv that is already split, and return the tool result it produced.
///
/// the per-command tools build their argv from a schema rather than from a line, so they arrive
/// here directly: quoting a json payload into a string only to take the quotes back off is a
/// round trip with nothing to gain and an escaping bug to lose.
pub(crate) async fn run(
    client: &Client,
    capture: &mut OutputCapture,
    tokens: Vec<String>,
    json: bool,
    timeout: Duration,
    api_base_url: &str,
    label: &str,
) -> Value {
    if let Some(blocked) = blocked_for(&tokens) {
        return protocol::text_result(
            format!(
                "`{}` cannot be run over mcp: {}",
                blocked_name(blocked),
                blocked.reason
            ),
            true,
        );
    }

    let tokens = with_json_flag(tokens, json);
    let parsed = match repl::parse(&tokens) {
        Ok(parsed) => parsed,
        // clap's rendered error carries the usage line and the nearest valid value, which is
        // exactly what the caller needs to fix the call itself.
        Err(failure) => return protocol::text_result(failure.to_string(), true),
    };

    // whatever a previous command left unread is not part of this result.
    let _ = capture.take();

    let json_output = parsed.json || json;
    let outcome = tokio::time::timeout(
        timeout,
        dispatch(client, &parsed.command, api_base_url, json_output),
    )
    .await;
    let output = capture.take();

    match outcome {
        Ok(Ok(())) => protocol::output_result(&output, false),
        Ok(Err(failure)) => protocol::text_result(
            match output.trim().is_empty() {
                true => failure.to_string(),
                false => format!("{output}\nerror: {failure}"),
            },
            true,
        ),
        Err(_) => protocol::text_result(
            format!(
                "{output}\nerror: `{label}` did not finish within {} seconds; \
                 raise `timeout_seconds` if it is expected to take longer",
                timeout.as_secs()
            ),
            true,
        ),
    }
}

/// dispatch a parsed command, including the two that run offline.
///
/// `workflows test` and `functions validate` need no web service, and `main` routes them around the
/// authenticated client for that reason. the same routing is repeated here so they work over MCP
/// with the server unreachable, which is exactly when a dry run is most useful.
async fn dispatch(
    client: &Client,
    command: &Commands,
    api_base_url: &str,
    json_output: bool,
) -> crate::commands::Result<()> {
    match command {
        Commands::Workflows {
            command:
                WorkflowCommands::Test {
                    file,
                    tests,
                    filter,
                },
        } => crate::commands::workflows_test(file, tests, filter.as_deref(), json_output),
        Commands::Functions {
            command: FunctionCommands::Validate { path },
        } => crate::commands::functions_validate(path, json_output),
        // The cycle is real: `run_command` can reach `mcp serve`, which reaches back here.
        // Box the future so its type does not contain itself. `mcp` is refused above, so the
        // recursion cannot actually happen, but the compiler has to be told.
        command => {
            let dispatched: std::pin::Pin<
                Box<dyn std::future::Future<Output = crate::commands::Result<()>> + '_>,
            > = Box::pin(run_command(client, command, api_base_url, json_output));
            dispatched.await
        }
    }
}

/// the blocked verb whose path prefixes the line, longest first.
///
/// longest-first is what lets `runs watch` be refused while `runs` stays open: a shorter path never
/// shadows a longer one that also matches.
pub(super) fn blocked_for(tokens: &[String]) -> Option<&'static Blocked> {
    let words: Vec<&String> = tokens
        .iter()
        .take_while(|token| !token.starts_with('-'))
        .collect();
    BLOCKED
        .iter()
        .filter(|blocked| {
            blocked.path.len() <= words.len()
                && blocked
                    .path
                    .iter()
                    .enumerate()
                    .all(|(index, word)| words[index] == word)
        })
        .max_by_key(|blocked| blocked.path.len())
}

fn blocked_name(blocked: &Blocked) -> String {
    blocked.path.join(" ")
}

/// append `--json` unless the caller already wrote it.
fn with_json_flag(mut tokens: Vec<String>, json: bool) -> Vec<String> {
    if json && !tokens.iter().any(|token| token == "--json") {
        tokens.push("--json".to_string());
    }
    tokens
}

#[cfg(test)]
#[path = "exec_tests.rs"]
mod tests;
