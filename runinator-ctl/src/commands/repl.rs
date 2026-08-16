//! what a `:`-prefixed console line means.
//!
//! the console repl accepts the whole `runinatorctl` command surface, and it does so by handing the
//! line to the same clap parser the process uses rather than by keeping a second table of verbs. a
//! command added to `Commands` is therefore reachable from the repl the day it is added, with the
//! same flags, defaults, and help text — a hand-maintained mirror is the copy nobody updates.

use clap::{CommandFactory, Parser};

use crate::cli::Commands;
use crate::commands::{Result, err};

/// the console-local verbs, which are handled before the line reaches clap.
///
/// these are the ones that only mean something inside a session — they move between sessions, read
/// the durable notebook, or act on the cell that just ran — so they have no command-line
/// counterpart to defer to.
pub(super) const META_COMMANDS: &[(&str, &str)] = &[
    (
        ":help",
        "list console verbs, or `:help <command>` for one command's flags",
    ),
    (":sessions", "list personal sessions"),
    (":new <name>", "create and use a session"),
    (":use <name|uuid>", "switch sessions"),
    (":history", "show durable cells"),
    (":bindings", "show the current scope"),
    (":cancel [cell-uuid]", "cancel durable remote work"),
    (":replay [cell-uuid]", "run a settled cell again"),
    (
        ":run workflow <name> [with <json>]",
        "start and follow a workflow run",
    ),
    (
        ":run pipeline <name> [with <json>]",
        "start and follow a pipeline run",
    ),
    (
        ":invoke <package.export> [alias <name>|version <n>] [with <json>]",
        "call a packaged function",
    ),
    (":clear", "clear the screen"),
    (":exit", "leave the console"),
];

/// the `runinatorctl` command line as the repl sees it: no binary name, and `--json` per command
/// rather than per process.
#[derive(Debug, Parser)]
#[command(
    name = "runinatorctl",
    no_binary_name = true,
    disable_help_subcommand = true,
    about = "Every runinatorctl command, prefixed with `:` inside the console"
)]
pub(super) struct ReplCommand {
    /// Print this command's output as json.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Commands,
}

/// split a line the way a shell would: whitespace separates arguments, quotes group them.
///
/// json arguments are the reason this is not `split_whitespace`: `:settings set scope name '{"a":
/// 1}'` has to survive as one argument, spaces and all.
pub(super) fn tokenize(line: &str) -> Result<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for character in line.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        // a backslash escapes inside double quotes and outside quotes, but not inside single quotes,
        // which is what lets a windows path or a json string be pasted verbatim in `'...'`.
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            has_token = true;
            continue;
        }
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if matches!(character, '\'' | '"') => {
                quote = Some(character);
                has_token = true;
            }
            None if character.is_whitespace() => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            None => {
                current.push(character);
                has_token = true;
            }
        }
    }

    if quote.is_some() {
        return Err(err("unterminated quote"));
    }
    if escaped {
        return Err(err("line ends with a dangling backslash"));
    }
    if has_token {
        tokens.push(current);
    }
    Ok(tokens)
}

/// parse a `:`-prefixed line into a command-line command.
///
/// the leading `:` is already stripped by the caller; what arrives here is the argument vector.
pub(super) fn parse(tokens: &[String]) -> Result<ReplCommand> {
    ReplCommand::try_parse_from(tokens).map_err(|error| err(error.render().to_string()))
}

/// the console verb list, followed by the command-line surface.
pub(super) fn help(topic: Option<&str>) -> Result<String> {
    let mut root = ReplCommand::command();
    let Some(topic) = topic else {
        let mut text = String::from("console commands:\n");
        for (verb, summary) in META_COMMANDS {
            text.push_str(&format!("  {verb:<44} {summary}\n"));
        }
        text.push_str("\nevery runinatorctl command works too, prefixed with `:`\n\n");
        text.push_str(&root.render_long_help().to_string());
        return Ok(text);
    };

    let topic = topic.trim_start_matches(':');
    let subcommand = root
        .get_subcommands_mut()
        .find(|candidate| candidate.get_name() == topic)
        .ok_or_else(|| err(format!("no console command '{topic}'")))?;
    Ok(subcommand.render_long_help().to_string())
}

/// true when Enter should submit the buffer rather than open another line.
///
/// a `:` command is always one line. WDL is not: an open brace, bracket, paren, or quote means the
/// author is mid-construct, and submitting there would send a fragment that cannot compile.
pub(crate) fn is_submittable(source: &str) -> bool {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with(':') {
        return true;
    }
    is_balanced(source) && !source.trim_end().ends_with('\\')
}

/// delimiters closed and quotes finished, ignoring anything escaped or inside a quote.
pub(crate) fn is_balanced(source: &str) -> bool {
    let mut stack = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    for character in source.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if let Some(open) = quote {
            if character == open {
                quote = None;
            }
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            continue;
        }
        match character {
            '{' | '[' | '(' => stack.push(character),
            '}' if stack.last() == Some(&'{') => {
                stack.pop();
            }
            ']' if stack.last() == Some(&'[') => {
                stack.pop();
            }
            ')' if stack.last() == Some(&'(') => {
                stack.pop();
            }
            _ => {}
        }
    }
    quote.is_none() && stack.is_empty()
}

/// what `Tab` offers at a point in a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Completion {
    /// byte offset where the word being replaced starts.
    pub start: usize,
    /// the candidates, already narrowed to what has been typed.
    pub options: Vec<String>,
}

/// complete the last word of `line`.
///
/// only `:` lines complete. a bare line is WDL, and offering `settings` to someone typing an
/// expression would be worse than offering nothing.
pub(crate) fn complete(line: &str) -> Completion {
    let trimmed = line.trim_start();
    if !trimmed.starts_with(':') {
        return Completion {
            start: line.len(),
            options: Vec::new(),
        };
    }

    // everything after the `:`, and where in the buffer it starts.
    let body_start = line.len() - trimmed.len() + ':'.len_utf8();
    let body = &line[body_start..];
    // the word being typed. a line ending in whitespace starts a new word, so the prefix is empty
    // and every word so far counts as context.
    let (prefix, start) = match body.rfind(char::is_whitespace) {
        Some(at) => (&body[at + 1..], body_start + at + 1),
        None => (body, body_start),
    };
    let typed = tokenize(&body[..start - body_start]).unwrap_or_default();

    Completion {
        start,
        options: candidates(&typed, prefix),
    }
}

// what may follow the words already typed.
fn candidates(typed: &[String], prefix: &str) -> Vec<String> {
    if prefix.starts_with("--") {
        return matching(flag_names(typed), prefix);
    }
    match typed {
        // the first word is either a console verb or a command-line verb, and the `:` the line
        // already carries serves both.
        [] => matching(first_words(), prefix),
        [verb] => matching(subcommand_names(verb), prefix),
        _ => Vec::new(),
    }
}

// console verbs (without their `:` and argument placeholders) plus the command-line verbs.
fn first_words() -> Vec<String> {
    META_COMMANDS
        .iter()
        .filter_map(|(verb, _)| verb.split_whitespace().next())
        .map(|verb| verb.trim_start_matches(':').to_string())
        .chain(command_names())
        .collect()
}

fn matching(mut names: Vec<String>, prefix: &str) -> Vec<String> {
    names.retain(|name| name.starts_with(prefix));
    names.sort();
    names.dedup();
    names
}

/// the top-level command-line verbs, for completion and error messages.
pub(super) fn command_names() -> Vec<String> {
    ReplCommand::command()
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_string())
        .collect()
}

/// the subcommand names under one verb, for completion.
pub(super) fn subcommand_names(verb: &str) -> Vec<String> {
    ReplCommand::command()
        .get_subcommands()
        .find(|subcommand| subcommand.get_name() == verb)
        .map(|subcommand| {
            subcommand
                .get_subcommands()
                .map(|nested| nested.get_name().to_string())
                .collect()
        })
        .unwrap_or_default()
}

/// the long flags accepted at a point in the command tree, for completion.
pub(super) fn flag_names(path: &[String]) -> Vec<String> {
    let root = ReplCommand::command();
    let mut current = &root;
    for segment in path {
        match current
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == segment)
        {
            Some(next) => current = next,
            None => break,
        }
    }
    current
        .get_arguments()
        .filter_map(|argument| argument.get_long())
        .map(|long| format!("--{long}"))
        .collect()
}

#[cfg(test)]
#[path = "repl_tests.rs"]
mod tests;
