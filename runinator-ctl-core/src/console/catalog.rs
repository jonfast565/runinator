//! every verb the console answers, flattened into one list.
//!
//! the command-line half of the list is *derived*: it is walked out of the same clap tree the
//! process parses with, so a `Commands` variant added today is listed, completed, and explained by
//! `:help` today. only the console-local verbs are declared here, because they have no clap
//! counterpart to read them from.

use clap::{Arg, ArgAction, Command, CommandFactory};

use super::ReplCommand;

/// a console-local verb, declared rather than derived.
pub struct MetaCommand {
    /// the words that select it, e.g. `["run", "workflow"]`.
    pub path: &'static [&'static str],
    /// the full call shape, shown by `:help`.
    pub usage: &'static str,
    pub summary: &'static str,
    /// what that word is, shown when it cannot be completed.
    pub hint: &'static str,
    /// flags that take no value, so `--debug foo` does not read `foo` as the value of `--debug`.
    pub booleans: &'static [&'static str],
}

const fn meta(
    path: &'static [&'static str],
    usage: &'static str,
    summary: &'static str,
    hint: &'static str,
) -> MetaCommand {
    MetaCommand {
        path,
        usage,
        summary,
        hint,
        booleans: &[],
    }
}

/// the console-local verbs, handled before a line reaches clap.
///
/// these are the ones that only mean something inside a session — they move between sessions, read
/// the durable notebook, or act on the cell that just ran — so they have no command-line
/// counterpart to defer to.
pub const META_COMMANDS: &[MetaCommand] = &[
    meta(
        &["help"],
        "help [command]",
        "list the commands, or show one command's arguments",
        "<command>",
    ),
    meta(
        &["clear"],
        "clear",
        "clear the screen; the session's cells and scope are untouched",
        "",
    ),
    meta(&["sessions"], "sessions", "list your console sessions", ""),
    meta(
        &["new"],
        "new <name>",
        "create a session and switch to it",
        "<name>",
    ),
    meta(&["use"], "use <name|id>", "switch sessions", "<name|id>"),
    meta(
        &["history"],
        "history",
        "show the durable cells in this session",
        "",
    ),
    meta(
        &["bindings"],
        "bindings",
        "show what this session's scope resolves to",
        "",
    ),
    meta(
        &["functions"],
        "functions",
        "show the active REXRAP function library for this session",
        "",
    ),
    meta(
        &["cancel"],
        "cancel [cell-id]",
        "cancel the durable run behind an effectful cell",
        "<cell-id>",
    ),
    meta(
        &["replay"],
        "replay [cell-id]",
        "run a settled cell again against the current scope",
        "<cell-id>",
    ),
    MetaCommand {
        path: &["run", "workflow"],
        usage: "run workflow <workflow> [--param KEY=VALUE] [--debug] [--name NAME] [with <json>]",
        summary: "start a workflow run and follow it",
        hint: "<workflow>  workflow id or name",
        booleans: &["debug"],
    },
    meta(
        &["run", "pipeline"],
        "run pipeline <pipeline> [--param KEY=VALUE] [with <json>]",
        "start a pipeline run and follow it",
        "<pipeline>  pipeline id or name",
    ),
    meta(
        &["invoke"],
        "invoke <package.export> [--alias NAME | --version N] [--input JSON]",
        "call a packaged function and print what it returned",
        "<package.export>",
    ),
    meta(&["exit"], "exit", "leave the console", ""),
];

/// one console verb: the words that select it, how it is called, and what it does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEntry {
    pub path: Vec<String>,
    pub usage: String,
    pub summary: String,
    /// the console answers this itself; everything else is dispatched as a command line.
    pub console_local: bool,
}

impl CommandEntry {
    /// the path as one word, which is how `:help` names a command.
    pub fn name(&self) -> String {
        self.path.join(" ")
    }
}

/// every verb, console-local first and then the command-line surface in declaration order.
///
/// built once: walking the clap tree is not free, and completion asks for this on every keystroke
/// that reaches `Tab`.
pub fn catalog() -> &'static [CommandEntry] {
    static CATALOG: std::sync::OnceLock<Vec<CommandEntry>> = std::sync::OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut entries: Vec<CommandEntry> = META_COMMANDS
            .iter()
            .map(|meta| CommandEntry {
                path: meta.path.iter().map(|word| (*word).to_string()).collect(),
                usage: meta.usage.to_string(),
                summary: meta.summary.to_string(),
                console_local: true,
            })
            .collect();
        collect(&ReplCommand::command(), &mut Vec::new(), &mut entries);
        entries
    })
}

/// the console-local verb whose path prefixes `tokens`, longest path first.
///
/// longest-first is what lets `run workflow` and a future bare `run` coexist: a shorter path never
/// shadows a longer one that also matches.
pub fn match_meta(tokens: &[String]) -> Option<&'static MetaCommand> {
    META_COMMANDS
        .iter()
        .filter(|meta| {
            meta.path.len() <= tokens.len()
                && meta
                    .path
                    .iter()
                    .enumerate()
                    .all(|(index, word)| tokens[index] == *word)
        })
        .max_by_key(|meta| meta.path.len())
}

// a parent with subcommands is not itself callable, so only leaves are listed — the same thing the
// web console's catalog does with its `path` arrays.
fn collect(command: &Command, path: &mut Vec<String>, into: &mut Vec<CommandEntry>) {
    for subcommand in command.get_subcommands() {
        path.push(subcommand.get_name().to_string());
        if subcommand.get_subcommands().next().is_some() {
            collect(subcommand, path, into);
        } else {
            into.push(CommandEntry {
                path: path.clone(),
                usage: usage(subcommand, path),
                summary: summary(subcommand),
                console_local: false,
            });
        }
        path.pop();
    }
}

/// the clap command a path selects, for reading its arguments back.
pub fn find(path: &[String]) -> Option<Command> {
    let mut current = ReplCommand::command();
    for segment in path {
        let next = current
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == segment)?
            .clone();
        current = next;
    }
    Some(current)
}

/// the arguments one command takes, as `(label, help)` pairs for `:help <command>`.
pub fn arguments(path: &[String]) -> Vec<(String, String)> {
    let Some(command) = find(path) else {
        return Vec::new();
    };
    command
        .get_arguments()
        .filter(|argument| listed(argument))
        .map(|argument| (label(argument), describe(argument)))
        .collect()
}

// what an argument is. most carry a doc comment; the ones that do not still have a closed set or a
// default worth saying, which beats an empty column.
pub fn describe(argument: &Arg) -> String {
    if let Some(help) = argument.get_help() {
        return one_line(&help.to_string());
    }
    // a boolean flag reports `true, false` as its possible values, which says nothing.
    let values = possible_values(argument);
    if takes_value(argument) && !values.is_empty() {
        return format!("one of {}", values.join(", "));
    }
    match argument.get_default_values() {
        [] => String::new(),
        [default] => format!("defaults to {}", default.to_string_lossy()),
        defaults => format!(
            "defaults to {}",
            defaults
                .iter()
                .map(|value| value.to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

/// the long flags accepted at a point in the command tree.
pub fn flag_names(path: &[String]) -> Vec<String> {
    walk(path)
        .into_iter()
        .flat_map(|command| {
            command
                .get_arguments()
                .filter_map(|argument| argument.get_long())
                .map(|long| format!("--{long}"))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// the values a flag accepts, when clap knows them as a closed set.
pub fn flag_values(path: &[String], flag: &str) -> Vec<String> {
    argument(path, flag)
        .map(|argument| possible_values(&argument))
        .unwrap_or_default()
}

/// true when a flag consumes the word after it.
pub fn flag_takes_value(path: &[String], flag: &str) -> bool {
    argument(path, flag).is_some_and(|argument| takes_value(&argument))
}

/// what to type after a flag, as `label` and its help.
pub fn flag_hint(path: &[String], flag: &str) -> Option<(String, String)> {
    let argument = argument(path, flag)?;
    Some((
        format!("--{flag} <{}>", value_name(&argument)),
        describe(&argument),
    ))
}

/// what to type at the given positional slot, as `label` and its help.
pub fn positional_hint(path: &[String], position: usize) -> Option<(String, String)> {
    let command = find(path)?;
    let argument = command
        .get_arguments()
        .filter(|argument| argument.is_positional() && listed(argument))
        .nth(position)?;
    Some((label(argument), describe(argument)))
}

/// the values a positional accepts, when clap knows them as a closed set.
pub fn positional_values(path: &[String], position: usize) -> Vec<String> {
    let Some(command) = find(path) else {
        return Vec::new();
    };
    command
        .get_arguments()
        .filter(|argument| argument.is_positional() && listed(argument))
        .nth(position)
        .map(possible_values)
        .unwrap_or_default()
}

// a flag may be declared on the command itself or inherited from an ancestor (`--json` is global),
// so the lookup walks the whole path rather than only its last segment.
fn argument(path: &[String], flag: &str) -> Option<Arg> {
    let wanted = flag.trim_start_matches('-');
    walk(path).into_iter().rev().find_map(|command| {
        command
            .get_arguments()
            .find(|argument| argument.get_long() == Some(wanted))
            .cloned()
    })
}

// the root and every command along `path` that exists, stopping at the first word that is not one.
fn walk(path: &[String]) -> Vec<Command> {
    let mut current = ReplCommand::command();
    let mut chain = vec![current.clone()];
    for segment in path {
        let Some(next) = current
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == segment)
            .cloned()
        else {
            break;
        };
        chain.push(next.clone());
        current = next;
    }
    chain
}

pub fn possible_values(argument: &Arg) -> Vec<String> {
    argument
        .get_possible_values()
        .iter()
        .map(|value| value.get_name().to_string())
        .collect()
}

fn usage(command: &Command, path: &[String]) -> String {
    let mut parts = vec![path.join(" ")];
    parts.extend(
        command
            .get_arguments()
            .filter(|argument| listed(argument))
            .map(label),
    );
    parts.join(" ")
}

// `<NAME>` for something required, `[NAME]` for something optional — the bracket convention the web
// console's usage strings already use.
fn label(argument: &Arg) -> String {
    let body = match argument.get_long() {
        Some(long) if takes_value(argument) => format!("--{long} {}", value_name(argument)),
        Some(long) => format!("--{long}"),
        None => value_name(argument),
    };
    if argument.is_required_set() {
        format!("<{body}>")
    } else {
        format!("[{body}]")
    }
}

// a short closed set reads better inline than the argument's name does: `--kind worker|waker` says
// what to type, where `--kind KIND` only says what it is called.
fn value_name(argument: &Arg) -> String {
    let values = possible_values(argument);
    let joined = values.join("|");
    if !values.is_empty() && joined.len() <= 28 {
        return joined;
    }
    argument
        .get_value_names()
        .and_then(|names| names.first())
        .map(|name| name.to_string())
        .unwrap_or_else(|| argument.get_id().as_str().to_uppercase())
}

pub fn takes_value(argument: &Arg) -> bool {
    !matches!(
        argument.get_action(),
        ArgAction::SetTrue | ArgAction::SetFalse | ArgAction::Count | ArgAction::Help
    )
}

// `--help` is clap's, and `--json` is global and explained once in the help preamble rather than
// repeated on all ninety commands.
pub fn listed(argument: &Arg) -> bool {
    !argument.is_hide_set() && !matches!(argument.get_id().as_str(), "help" | "json" | "version")
}

fn summary(command: &Command) -> String {
    command
        .get_about()
        .map(|about| one_line(&about.to_string()))
        .unwrap_or_default()
}

// clap help text is written as prose over several lines; a table cell takes the first sentence.
fn one_line(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match collapsed.split_once(". ") {
        Some((first, _)) => format!("{first}."),
        None => collapsed,
    }
}

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod tests;
