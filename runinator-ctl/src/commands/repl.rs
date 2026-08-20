//! what a `:`-prefixed console line means.
//!
//! the console repl accepts the whole `runinatorctl` command surface, and it does so by handing the
//! line to the same clap parser the process uses rather than by keeping a second table of verbs. a
//! command added to `Commands` is therefore reachable from the repl the day it is added, with the
//! same flags, defaults, and help text — a hand-maintained mirror is the copy nobody updates.
//!
//! a line is read the same way in both consoles: tokenize, take the longest command path that
//! prefixes the tokens, then split what is left into positionals and flags. the console-local verbs
//! are matched here; everything else is handed to clap with its arguments already separated.

use std::collections::BTreeMap;

use clap::{CommandFactory, Parser};

use super::catalog::{self, CommandEntry, MetaCommand};
use crate::cli::Commands;
use crate::commands::{Result, err};
use crate::output;

/// the `runinatorctl` command line as the repl sees it: no binary name, and `--json` per command
/// rather than per process.
#[derive(Debug, Parser)]
#[command(
    name = "runinatorctl",
    no_binary_name = true,
    disable_help_subcommand = true,
    about = "Every runinatorctl command, prefixed with `:` inside the console"
)]
pub(crate) struct ReplCommand {
    /// Print this command's output as json.
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Commands,
}

/// one word of a line, both as the shell would read it and as it was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Token {
    /// the argument value: quotes removed, escapes applied.
    pub text: String,
    /// the source text, quotes and all.
    ///
    /// a bare `{"width": 320}` reads as `{width: 320}` once quoting is applied, which is not json
    /// any more. the `… with <json>` tail is therefore taken from what was written rather than from
    /// what was read, so an unquoted payload survives.
    pub raw: String,
}

/// split a line the way a shell would: whitespace separates arguments, quotes group them.
///
/// json arguments are the reason this is not `split_whitespace`: `:settings set scope name '{"a":
/// 1}'` has to survive as one argument, spaces and all.
pub(super) fn scan(line: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut start = None;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (at, character) in line.char_indices() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        // a backslash escapes inside double quotes and outside quotes, but not inside single quotes,
        // which is what lets a windows path or a json string be pasted verbatim in `'...'`.
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            start.get_or_insert(at);
            continue;
        }
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if matches!(character, '\'' | '"') => {
                quote = Some(character);
                start.get_or_insert(at);
            }
            None if character.is_whitespace() => {
                if let Some(from) = start.take() {
                    tokens.push(Token {
                        text: std::mem::take(&mut current),
                        raw: line[from..at].to_string(),
                    });
                }
            }
            None => {
                current.push(character);
                start.get_or_insert(at);
            }
        }
    }

    if quote.is_some() {
        return Err(err("unterminated quote"));
    }
    if escaped {
        return Err(err("line ends with a dangling backslash"));
    }
    if let Some(from) = start {
        tokens.push(Token {
            text: current,
            raw: line[from..].to_string(),
        });
    }
    Ok(tokens)
}

/// the words of a line, as a command parser reads them.
pub(super) fn tokenize(line: &str) -> Result<Vec<String>> {
    Ok(scan(line)?.into_iter().map(|token| token.text).collect())
}

/// positionals and flags, split apart.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Arguments {
    pub args: Vec<String>,
    /// the same positionals as they were written, for the one caller that needs unquoted json.
    raw_args: Vec<String>,
    /// a repeated flag keeps every value, since several commands take a repeatable `--param`.
    pub flags: BTreeMap<String, Vec<String>>,
}

impl Arguments {
    /// everything written after a bare `word` positional, verbatim.
    ///
    /// this is how `… with {"a": 1}` is read: as source rather than as arguments, so a payload
    /// nobody quoted is still the json it looks like.
    pub fn raw_after(&self, word: &str) -> Option<String> {
        let at = self.args.iter().position(|value| value == word)?;
        Some(self.raw_args[at + 1..].join(" "))
    }

    /// a positional by index.
    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(String::as_str)
    }

    /// a positional, or the failure that names what was missing.
    pub fn required(&self, index: usize, name: &str) -> Result<&str> {
        self.arg(index)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| err(format!("{name} is required")))
    }

    /// the last value given for a flag.
    pub fn flag(&self, name: &str) -> Option<&str> {
        self.flags
            .get(name)
            .and_then(|values| values.last())
            .map(String::as_str)
    }

    /// every value of a repeatable flag.
    pub fn flag_list(&self, name: &str) -> &[String] {
        self.flags.get(name).map(Vec::as_slice).unwrap_or_default()
    }

    /// true when the flag was present at all, whatever it carried.
    pub fn is_set(&self, name: &str) -> bool {
        self.flags.contains_key(name)
    }
}

/// split tokens into positionals and flags.
///
/// `--name value`, `--name=value`, and a bare `--name` are all accepted. a flag listed in
/// `booleans` never consumes the next token, which is what stops `--debug list` from reading `list`
/// as the value of `--debug`. this mirrors the web console's `parseArguments` deliberately: the two
/// consoles accept the same lines, so they have to disagree about nothing.
pub(super) fn parse_arguments(tokens: &[Token], booleans: &[&str]) -> Arguments {
    let mut parsed = Arguments::default();
    let mut index = 0;
    while index < tokens.len() {
        let token = &tokens[index].text;
        index += 1;
        let Some(body) = token.strip_prefix("--") else {
            parsed.args.push(token.clone());
            parsed.raw_args.push(tokens[index - 1].raw.clone());
            continue;
        };
        if body.is_empty() {
            continue;
        }
        if let Some((name, value)) = body.split_once('=') {
            parsed
                .flags
                .entry(name.to_string())
                .or_default()
                .push(value.to_string());
            continue;
        }
        let next = tokens.get(index);
        let boolean = body == "json"
            || booleans.contains(&body)
            || next.is_none()
            || next.is_some_and(|token| token.text.starts_with("--"));
        if boolean {
            parsed.flags.entry(body.to_string()).or_default();
            continue;
        }
        parsed
            .flags
            .entry(body.to_string())
            .or_default()
            .push(tokens[index].text.clone());
        index += 1;
    }
    parsed
}

/// the console-local verb a line selects, and the arguments left over.
pub(super) struct MetaMatch {
    pub command: &'static MetaCommand,
    pub arguments: Arguments,
}

/// match a scanned line against the console-local verbs.
pub(super) fn match_meta(tokens: &[Token]) -> Option<MetaMatch> {
    let words: Vec<String> = tokens.iter().map(|token| token.text.clone()).collect();
    let command = catalog::match_meta(&words)?;
    Some(MetaMatch {
        arguments: parse_arguments(&tokens[command.path.len()..], command.booleans),
        command,
    })
}

/// parse a `:`-prefixed line into a command-line command.
///
/// the leading `:` is already stripped by the caller; what arrives here is the argument vector.
pub(super) fn parse(tokens: &[String]) -> Result<ReplCommand> {
    ReplCommand::try_parse_from(tokens).map_err(|error| err(error.render().to_string()))
}

/// how an unrecognised first word reports itself, with the nearest verb offered.
pub(super) fn unknown_command(word: &str) -> String {
    let mut message = format!("unknown console command '{word}'");
    if let Some(nearest) = nearest_command(word) {
        message.push_str(&format!("; did you mean ':{nearest}'?"));
    }
    message.push_str(" try :help");
    message
}

// the closest first word by edit distance, when it is close enough to be a typo rather than a
// different word entirely.
fn nearest_command(word: &str) -> Option<String> {
    let limit = 1 + word.chars().count() / 3;
    first_words()
        .into_iter()
        .map(|candidate| {
            let distance = edit_distance(word, &candidate);
            (distance, candidate)
        })
        .filter(|(distance, _)| *distance <= limit)
        .min_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, candidate)| candidate)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    let mut current = vec![0; right.len() + 1];
    for (row, from) in left.chars().enumerate() {
        current[0] = row + 1;
        for (column, to) in right.iter().enumerate() {
            let cost = usize::from(from != *to);
            current[column + 1] = (previous[column] + cost)
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[right.len()]
}

/// the command list, or one command's call shape and arguments.
///
/// the format is the web console's: a hint line, then a table of every command against what it
/// does. `:help <command>` narrows to the commands whose path starts with the topic and adds the
/// arguments each one takes.
pub(super) fn help(topic: Option<&str>) -> Result<String> {
    let entries = catalog::catalog();
    let Some(topic) = topic.map(|topic| topic.trim().trim_start_matches(':')) else {
        let names: Vec<String> = entries
            .iter()
            .map(|entry| format!(":{}", entry.name()))
            .collect();
        let width = column_width(names.iter().map(String::as_str));
        let rows: Vec<Vec<String>> = entries
            .iter()
            .zip(names)
            .map(|(entry, name)| vec![name, output::truncate(&entry.summary, summary_width(width))])
            .collect();
        let mut text = String::from(
            "a bare line is REXRAP; a `:` line is a command. `:help <command>` for arguments.\n\
             every command also takes `--json` to print the raw payload.\n\n",
        );
        text.push_str(&output::table(&["command", "what it does"], &rows));
        return Ok(text);
    };

    let matches: Vec<&CommandEntry> = entries
        .iter()
        .filter(|entry| entry.name().starts_with(topic))
        .collect();
    if matches.is_empty() {
        return Err(err(unknown_command(topic)));
    }

    // one command gets the long form — its whole call shape on a line of its own, then every
    // argument. a prefix that matched several is a menu, and expanding all of them would bury it.
    if let [entry] = matches.as_slice() {
        let mut text = format!("{}\n{}\n", entry.usage, entry.summary);
        let arguments = catalog::arguments(&entry.path);
        if !arguments.is_empty() {
            let rows: Vec<Vec<String>> = arguments
                .into_iter()
                .map(|(label, help)| vec![label, help])
                .collect();
            text.push('\n');
            text.push_str(&output::table(&["argument", "what it is"], &rows));
        }
        return Ok(text);
    }

    let width = column_width(matches.iter().map(|entry| entry.usage.as_str()));
    let rows: Vec<Vec<String>> = matches
        .iter()
        .map(|entry| {
            vec![
                output::truncate(&entry.usage, width),
                output::truncate(&entry.summary, summary_width(width)),
            ]
        })
        .collect();
    Ok(output::table(&["usage", "what it does"], &rows))
}

// the widest cell, capped at half the terminal: one command whose flags fill a paragraph would
// otherwise push every summary in the table off the right of the screen.
fn column_width<'a>(values: impl Iterator<Item = &'a str>) -> usize {
    let widest = values.map(|value| value.chars().count()).max().unwrap_or(0);
    widest.min(output::terminal_width() / 2).max(12)
}

// the summary column gets whatever the terminal has left.
fn summary_width(first: usize) -> usize {
    output::terminal_width().saturating_sub(first + 3).max(24)
}

/// true when Enter should submit the buffer rather than open another line.
///
/// a `:` command is always one line. REXRAP is not: an open brace, bracket, paren, or quote means the
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Completion {
    /// byte offset where the word being replaced starts.
    pub start: usize,
    /// the candidates, already narrowed to what has been typed.
    pub options: Vec<String>,
    /// what belongs here when there is nothing to offer: the value's name and what it means.
    pub hint: Option<String>,
}

/// complete the last word of `line`.
///
/// only `:` lines complete. a bare line is REXRAP, and offering `settings` to someone typing an
/// expression would be worse than offering nothing.
pub(crate) fn complete(line: &str) -> Completion {
    let trimmed = line.trim_start();
    if !trimmed.starts_with(':') {
        return Completion {
            start: line.len(),
            ..Completion::default()
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

    let (options, hint) = candidates(&typed, prefix);
    Completion {
        start,
        options: matching(options, prefix),
        hint,
    }
}

// what may follow the words already typed, and what to say when nothing can be offered.
fn candidates(typed: &[String], prefix: &str) -> (Vec<String>, Option<String>) {
    // a value for the flag just typed comes before anything else: after `--kind ` the answer is a
    // kind, never a subcommand.
    if let Some(flag) = pending_flag(typed) {
        let path = command_path(typed);
        let values = catalog::flag_values(&path, &flag);
        if !values.is_empty() {
            return (values, None);
        }
        if !prefix.starts_with("--") {
            return (Vec::new(), catalog::flag_hint(&path, &flag).map(describe));
        }
    }
    if prefix.starts_with("--") {
        return (flag_names(typed), None);
    }

    // the next word of a command path, from every entry whose path still aligns with what has been
    // typed. this is the web console's `completions`, and it is why a three-word path completes.
    let entries = catalog::catalog();
    let mut words = Vec::new();
    for entry in entries {
        let aligns = entry
            .path
            .iter()
            .take(typed.len())
            .enumerate()
            .all(|(index, word)| typed[index] == *word);
        if aligns && let Some(next) = entry.path.get(typed.len()) {
            words.push(next.clone());
        }
    }
    if !words.is_empty() {
        return (words, None);
    }

    // the path is complete, so what follows is one of its values.
    let path = command_path(typed);
    let position = typed.len().saturating_sub(path.len());
    let values = catalog::positional_values(&path, position);
    if !values.is_empty() {
        return (values, None);
    }
    (Vec::new(), value_hint(typed, &path, position))
}

// the words of `typed` that name a command, which is what every lookup is relative to.
fn command_path(typed: &[String]) -> Vec<String> {
    catalog::catalog()
        .iter()
        .filter(|entry| {
            entry.path.len() <= typed.len()
                && entry
                    .path
                    .iter()
                    .enumerate()
                    .all(|(index, word)| typed[index] == *word)
        })
        .map(|entry| entry.path.clone())
        .max_by_key(Vec::len)
        .unwrap_or_default()
}

// what the next value is, for the hint line under the prompt.
fn value_hint(typed: &[String], path: &[String], position: usize) -> Option<String> {
    if let Some(meta) = catalog::match_meta(typed) {
        let hint = meta.hint.trim();
        return (!hint.is_empty() && position == 0).then(|| hint.to_string());
    }
    catalog::positional_hint(path, position).map(describe)
}

// a label and its help as one line, which is all the hint band has room for.
fn describe((label, help): (String, String)) -> String {
    if help.is_empty() {
        label
    } else {
        format!("{label}  {help}")
    }
}

// the flag whose value is being typed: the previous word is a long flag that takes one and does not
// already carry it inline.
fn pending_flag(typed: &[String]) -> Option<String> {
    let last = typed.last()?;
    let name = last.strip_prefix("--")?;
    if name.is_empty() || name.contains('=') {
        return None;
    }
    let path = command_path(typed);
    catalog::flag_takes_value(&path, name).then(|| name.to_string())
}

fn flag_names(typed: &[String]) -> Vec<String> {
    let mut names = catalog::flag_names(&command_path(typed));
    names.push("--json".to_string());
    names
}

fn matching(mut names: Vec<String>, prefix: &str) -> Vec<String> {
    names.retain(|name| name.starts_with(prefix));
    names.sort();
    names.dedup();
    names
}

// console verbs plus the command-line verbs, for the first word of a line.
fn first_words() -> Vec<String> {
    let mut words: Vec<String> = catalog::catalog()
        .iter()
        .filter_map(|entry| entry.path.first().cloned())
        .collect();
    words.sort();
    words.dedup();
    words
}

/// the top-level command-line verbs, for dispatch and error messages.
pub(super) fn command_names() -> Vec<String> {
    ReplCommand::command()
        .get_subcommands()
        .map(|subcommand| subcommand.get_name().to_string())
        .collect()
}

#[cfg(test)]
#[path = "repl_tests.rs"]
mod tests;
