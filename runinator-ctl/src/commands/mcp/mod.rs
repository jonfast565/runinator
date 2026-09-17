//! a model context protocol server over the `runinatorctl` command surface.
//!
//! this is the third console. the terminal console, the command center's Console tab, and this all
//! reach the same verbs through the same clap parser (`commands::repl` → `commands::run_command`),
//! which is what stops any of them growing a second, smaller table of commands that drifts. what
//! differs is who is typing: a person at a prompt, a person in a browser, and here a model over
//! JSON-RPC on stdin and stdout.
//!
//! the surface it advertises is the whole command line — one tool per command, derived in `schema`
//! from the clap tree, plus `runinator_exec` for a raw line and `runinator_help` for the catalog.
//! saved workflows can be exposed as tools too, behind `--workflow-tools`.
//!
//! the one thing this server has to be careful about is its own stdout: the command modules print
//! with plain `println!`, and a table written into the middle of a JSON-RPC frame would desynchronise
//! the client. `capture` takes stdout and stderr away from them and hands back a duplicate of the
//! real stdout, which is the only thing the protocol answers on.

mod capture;
mod exec;
mod protocol;
mod resources;
mod schema;
mod tools;
mod workflow_tools;

use std::io::Write;
use std::time::Duration;

use runinator_models::json;
use runinator_models::value::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

use self::capture::OutputCapture;
use self::protocol::{PARSE_ERROR, failure, internal_error, success};
use crate::commands::{Client, Result};

/// JSON-RPC's "no such method", for a request naming something this server does not implement.
const METHOD_NOT_FOUND: i64 = -32601;

/// how the server names itself to a client.
const SERVER_NAME: &str = "runinatorctl";

/// how long a command started by a per-command tool may run.
///
/// generous, because `workflows apply` on a large pack legitimately takes a while, and a tool call
/// that gives up early looks to the model exactly like a failure. `runinator_exec` takes an explicit
/// `timeout_seconds` for the rarer command that needs longer still.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

/// what the server was started with.

/// serve the protocol on stdin/stdout until the client closes the pipe.
pub(crate) async fn serve(client: &Client, api_base_url: &str, options: Options) -> Result<()> {
    // installed before the first frame is written: from here on `println!` goes to the scratch file
    // and `screen` is the protocol channel.
    let (mut capture, mut screen) = OutputCapture::install()?;
    let mut server = Server {
        client,
        api_base_url,
        options,
        capture: &mut capture,
    };

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let Some(response) = server.respond(&line).await else {
            continue;
        };
        writeln!(screen, "{response}")?;
        screen.flush()?;
    }

    // the standard streams go back before the process ends, so anything printed afterwards — a shutdown
    // error, a panic message — reaches the terminal rather than the scratch file.
    capture.restore();
    Ok(())
}

fn fence_mission_arguments(arguments: &Value, mission_id: Option<Uuid>) -> Value {
    let mut arguments = arguments.clone();
    if let Some(mission_id) = mission_id
        && let Some(arguments) = arguments.as_object_mut()
    {
        arguments.insert("id".into(), Value::String(mission_id.to_string()));
    }
    arguments
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;

mod options;
pub(crate) use options::Options;

mod server;
use server::Server;
