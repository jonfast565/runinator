//! the tools the server advertises.
//!
//! every `runinatorctl` command is a tool, built in `schema` from the clap tree — so the model sees
//! named, typed, documented arguments rather than a command line it has to compose, and a verb added
//! to `Commands` is a tool the day it is added. nothing here is written down twice.
//!
//! two tools sit in front of that set. `runinator_help` reads the same command catalog the console's
//! `:help` reads, for finding a verb without pulling its whole schema into the conversation; and
//! `runinator_exec` runs a raw command line, which is the escape hatch for a longer timeout, for
//! `--json` off, and for anything a schema cannot express.

use runinator_models::json;
use runinator_models::value::Value;

use super::protocol::{object_schema, optional_str, structured_result, text_result};
use super::schema;
use crate::output;
use runinator_ctl_core::console::catalog::{self, CommandEntry};

/// what the server tells a client it is, returned from `initialize`.
pub(crate) const INSTRUCTIONS: &str = "Runinator schedules and runs workflows across a distributed \
     runtime. This server is the `runinatorctl` control surface: there is one tool per command, \
     named `runinator_<command>_<subcommand>`, covering workflow packs and their authoring, runs \
     and their logs and artifacts, triggers and freeze windows, approvals, settings and secrets, \
     packaged functions, providers, replicas, and orgs. Workflows are authored in REXRAP and applied \
     as packs with `runinator_workflows_apply`; compilation happens here, not on the server. Use \
     `runinator_help` to find a command, and `runinator_exec` to run a raw command line when a \
     tool's schema does not fit.";

/// how the exec tool describes the surface it reaches, so a first call needs no round trip.
const TOP_LEVEL: &str = "workflows, pipelines, runs, nodes, triggers, freeze, approvals, \
                         artifacts, settings, functions, providers, replicas, agents, orgs, \
                         rexrap, status";

pub(crate) const HELP_TOOL: &str = "runinator_help";
pub(crate) const EXEC_TOOL: &str = "runinator_exec";

/// every tool: the two general ones, one per command, and the workflow tools when exposed.
///
/// help and exec come first deliberately — a client that truncates a long list keeps the two that
/// can still reach everything else.
pub(crate) fn definitions(workflow_tools: Vec<Value>) -> Vec<Value> {
    let mut tools = vec![help_tool(), exec_tool()];
    tools.extend(schema::definitions());
    tools.extend(workflow_tools);
    tools
}

fn help_tool() -> Value {
    json!({
        "name": HELP_TOOL,
        "description": format!(
            "List every runinatorctl command, or show one command's arguments. Each command also has \
             a tool of its own named `runinator_<command>_<subcommand>`; this is the index over them, \
             and is cheaper to read than every schema when the right verb is not yet known. The list \
             is walked out of the command parser itself, so it is never stale. Top-level groups: \
             {TOP_LEVEL}."
        ),
        "inputSchema": object_schema(
            &[(
                "command",
                json!({
                    "type": "string",
                    "description": "A command or a prefix of one, such as \"settings set\" or \
                                    \"workflows\". Omit to list everything.",
                }),
            )],
            &[],
        ),
        "annotations": {
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false,
        },
    })
}

fn exec_tool() -> Value {
    json!({
        "name": EXEC_TOOL,
        "description": format!(
            "Run one runinatorctl command written as a raw command line, and return what it printed. \
             Prefer the tool named after the command itself — `runinator_workflows_apply` and the \
             rest — which takes the same arguments as named, checked fields. This is the escape \
             hatch: use it for a command that needs longer than the default timeout, for output as \
             a human table rather than json, or for anything a schema does not express. Quoting \
             works as it does in a shell, so a json payload can be passed as one quoted argument. \
             Use `{HELP_TOOL}` for a command's exact flags. Top-level groups: {TOP_LEVEL}."
        ),
        "inputSchema": object_schema(
            &[
                (
                    "command",
                    json!({
                        "type": "string",
                        "description": "The command line without the `runinatorctl` prefix, e.g. \
                                        \"workflows apply packs/sdlc\" or \
                                        \"settings set github token --kind secret\".",
                    }),
                ),
                (
                    "json",
                    json!({
                        "type": "boolean",
                        "default": true,
                        "description": "Ask the command for its raw json payload rather than the \
                                        human table. A few commands print the same either way.",
                    }),
                ),
                (
                    "timeout_seconds",
                    json!({
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 3600,
                        "description": "How long to wait before giving up on the command.",
                    }),
                ),
            ],
            &["command"],
        ),
        "annotations": {
            // the surface reaches every verb, including the ones that delete, so the conservative
            // hints are the honest ones.
            "readOnlyHint": false,
            "destructiveHint": true,
            "idempotentHint": false,
            "openWorldHint": true,
        },
    })
}

/// answer `runinator_help`.
pub(crate) fn help(arguments: &Value) -> Value {
    let entries: Vec<&CommandEntry> = catalog::catalog()
        .iter()
        .filter(|entry| !entry.console_local)
        .collect();
    let Some(topic) = optional_str(arguments, "command") else {
        return listing(&entries);
    };

    let topic = topic.trim().trim_start_matches(':').to_string();
    let matches: Vec<&CommandEntry> = entries
        .iter()
        .copied()
        .filter(|entry| entry.name().starts_with(&topic))
        .collect();
    if matches.is_empty() {
        return text_result(
            format!(
                "no command starts with '{topic}'. call {HELP_TOOL} with no argument to list them."
            ),
            true,
        );
    }
    // one match gets the long form — its whole call shape, then every argument. a prefix that
    // matched several is a menu, and expanding all of them would bury it.
    if let [entry] = matches.as_slice() {
        return detail(entry);
    }
    listing(&matches)
}

fn listing(entries: &[&CommandEntry]) -> Value {
    let rows: Vec<Vec<String>> = entries
        .iter()
        .map(|entry| vec![entry.name(), entry.summary.clone()])
        .collect();
    let structured = Value::Array(
        entries
            .iter()
            .map(|entry| {
                json!({
                    "command": entry.name(),
                    "usage": entry.usage,
                    "summary": entry.summary,
                })
            })
            .collect(),
    );
    let text = format!(
        "pass one of these as `command` to {EXEC_TOOL}, with its arguments appended.\n\
         every command also takes `--json`.\n\n{}",
        output::table(&["command", "what it does"], &rows)
    );
    structured_result(text, structured, false)
}

fn detail(entry: &CommandEntry) -> Value {
    let arguments = catalog::arguments(&entry.path);
    let mut text = format!("{}\n{}\n", entry.usage, entry.summary);
    if !arguments.is_empty() {
        let rows: Vec<Vec<String>> = arguments
            .iter()
            .map(|(label, help)| vec![label.clone(), help.clone()])
            .collect();
        text.push('\n');
        text.push_str(&output::table(&["argument", "what it is"], &rows));
    }
    let structured = json!({
        "command": entry.name(),
        "usage": entry.usage,
        "summary": entry.summary,
        "arguments": arguments
            .iter()
            .map(|(label, help)| json!({ "argument": label, "description": help }))
            .collect::<Vec<_>>(),
    });
    structured_result(text, structured, false)
}

#[cfg(test)]
#[path = "tools_tests.rs"]
mod tests;
