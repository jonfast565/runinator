//! a model context protocol server over the `runinatorctl` command surface.
//!
//! this is the third console. the terminal console, the command center's Console tab, and this all
//! reach the same verbs through the same clap parser (`commands::repl` → `commands::run_command`),
//! which is what stops any of them growing a second, smaller table of commands that drifts. what
//! differs is who is typing: a person at a prompt, a person in a browser, and here a model over
//! json-rpc on stdin and stdout.
//!
//! the surface it advertises is the whole command line — one tool per command, derived in `schema`
//! from the clap tree, plus `runinator_exec` for a raw line and `runinator_help` for the catalog.
//! saved workflows can be exposed as tools too, behind `--workflow-tools`.
//!
//! the one thing this server has to be careful about is its own stdout: the command modules print
//! with plain `println!`, and a table written into the middle of a json-rpc frame would desynchronise
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

use self::capture::OutputCapture;
use self::protocol::{PARSE_ERROR, failure, internal_error, success};
use crate::commands::{Client, Result};

/// json-rpc's "no such method", for a request naming something this server does not implement.
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
pub(crate) struct Options {
    /// expose every saved workflow as a tool of its own.
    pub workflow_tools: bool,
    /// the default ceiling on one command.
    pub timeout: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            workflow_tools: false,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

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

struct Server<'a> {
    client: &'a Client,
    api_base_url: &'a str,
    options: Options,
    capture: &'a mut OutputCapture,
}

impl Server<'_> {
    /// the frame to write back, or nothing when the line was a notification.
    async fn respond(&mut self, line: &str) -> Option<Value> {
        let request: Value = match serde_json::from_str(line) {
            Ok(request) => request,
            Err(broken) => return Some(failure(Value::Null, PARSE_ERROR, broken.to_string())),
        };
        // a notification carries no id and takes no reply — `notifications/initialized` is the one
        // every client sends, and answering it is a protocol error.
        let id = request.get("id").cloned()?;
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(Value::Null);
        Some(self.dispatch(id, method, params).await)
    }

    async fn dispatch(&mut self, id: Value, method: &str, params: Value) -> Value {
        match method {
            "initialize" => success(id, self.initialize()),
            "ping" => success(id, json!({})),
            "tools/list" => success(id, json!({ "tools": self.tool_definitions().await })),
            "tools/call" => self.call(id, params).await,
            "resources/list" => success(
                id,
                json!({ "resources": resources::list(self.client).await }),
            ),
            "resources/templates/list" => {
                success(id, json!({ "resourceTemplates": resources::templates() }))
            }
            "resources/read" => match params.get("uri").and_then(Value::as_str) {
                Some(uri) => match resources::read(self.client, uri).await {
                    Ok(contents) => success(id, contents),
                    Err(message) => internal_error(id, message),
                },
                None => internal_error(id, "resources/read needs a 'uri'"),
            },
            method => failure(
                id,
                METHOD_NOT_FOUND,
                format!("this server does not implement '{method}'"),
            ),
        }
    }

    fn initialize(&self) -> Value {
        json!({
            "protocolVersion": protocol::PROTOCOL_VERSION,
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") },
            "capabilities": {
                "tools": { "listChanged": false },
                "resources": { "subscribe": false, "listChanged": false },
            },
            "instructions": tools::INSTRUCTIONS,
        })
    }

    /// the tools, which are the command surface plus whatever else is switched on.
    ///
    /// the workflow tools are the only ones that need the web service to list, so an unreachable
    /// server still advertises the command line — which is what the caller needs to find out *why*
    /// it is unreachable.
    async fn tool_definitions(&self) -> Vec<Value> {
        let workflows = match self.options.workflow_tools {
            true => self.client.fetch_workflows().await.unwrap_or_default(),
            false => Vec::new(),
        };
        tools::definitions(workflow_tools::definitions(workflows))
    }

    async fn call(&mut self, id: Value, params: Value) -> Value {
        let Some(name) = params.get("name").and_then(Value::as_str) else {
            return internal_error(id, "tools/call needs a 'name'");
        };
        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| Value::Object(Default::default()));
        // a tool that failed still answers with a result carrying `isError`; only a call that could
        // not be read at all is a transport error.
        success(id, self.tool(name, arguments).await)
    }

    async fn tool(&mut self, name: &str, arguments: Value) -> Value {
        if name == tools::HELP_TOOL {
            return tools::help(&arguments);
        }
        if name == tools::EXEC_TOOL {
            return self.exec_tool(&arguments).await;
        }
        if let Some(tool) = schema::find(name) {
            return self.command_tool(tool, &arguments).await;
        }
        if self.options.workflow_tools && workflow_tools::workflow_id_for(name).is_some() {
            return workflow_tools::call(self.client, name, arguments).await;
        }
        protocol::text_result(
            format!("no tool named '{name}'. call tools/list to see what there is."),
            true,
        )
    }

    async fn exec_tool(&mut self, arguments: &Value) -> Value {
        let command = match protocol::required_str(arguments, "command") {
            Ok(command) => command,
            Err(message) => return protocol::text_result(message, true),
        };
        // json is the default: a model reads a payload better than a table, and every command takes
        // the flag even when it prints the same either way.
        let json = arguments
            .get("json")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let timeout = arguments
            .get("timeout_seconds")
            .and_then(Value::as_i64)
            .filter(|seconds| *seconds > 0)
            .map(|seconds| Duration::from_secs(seconds as u64))
            .unwrap_or(self.options.timeout);
        exec::exec(
            self.client,
            self.capture,
            &command,
            json,
            timeout,
            self.api_base_url,
        )
        .await
    }

    async fn command_tool(&mut self, tool: &schema::CommandTool, arguments: &Value) -> Value {
        let line = match schema::command_line(tool, arguments) {
            Ok(line) => line,
            // a rejected argument is the model's to read and fix, so it comes back as a tool error
            // with the command's own argument names in it rather than as a transport failure.
            Err(message) => return protocol::text_result(message, true),
        };
        exec::run(
            self.client,
            self.capture,
            line,
            true,
            self.options.timeout,
            self.api_base_url,
            &tool.path.join(" "),
        )
        .await
    }
}
